//! The edit, play, return, edit loop, driven without a window, and the
//! render thread's promise that a slow scene never slows the UI.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use mscent_platform::{Event, Key};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, Quality};

use super::worker::Request;
use super::*;

const PANE: (u32, u32) = (480, 300);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// Ticks until the picture of `doc` is up and fully lit.
fn settle(studio: &mut Studio, doc: &Document) {
    let end = Instant::now() + Duration::from_secs(60);
    while studio.preview(doc) != Preview::Full {
        studio.tick(&[], PANE, doc);
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Ticks until a frame arrives (the UI thread never waits; the test does).
fn next_frame(studio: &mut Studio, doc: &Document, events: &[Event]) -> Frame {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let t = studio.tick(events, PANE, doc);
        if let Some(f) = t.frame {
            return f;
        }
        assert!(Instant::now() < end, "no frame within 30 s");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn key(c: Key, pressed: bool) -> Event {
    Event::Key { code: c, pressed }
}

fn rgba_of(fb: &Framebuffer) -> Vec<u8> {
    fb.pixels()
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255])
        .collect()
}

#[test]
fn editor_view_is_pixel_identical_to_the_renderer_for_the_same_camera() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    settle(&mut studio, &doc);
    let mut cam = FlyCam::new(mscent_math::Vec3::new(2.0, -3.0, 2.0), 0.7, -0.2);
    cam.roll = 0.25;
    studio.set_camera(cam.clone());

    let level = doc.to_level().expect("valid");
    let materials = miniscent_game::assets::load_materials(None).expect("textures");
    let direct =
        LevelRenderer::from_level(&level, materials, &BuildConfig::default()).expect("builds");
    // Frames drawn before the camera command arrived may come first.
    let end = Instant::now() + Duration::from_secs(30);
    let (mine, expected) = loop {
        let f = next_frame(&mut studio, &doc, &[]);
        let mut fb = Framebuffer::new(f.w, f.h).expect("fb");
        direct
            .render(&mut fb, &cam.view(), Quality::High, direct.actors(), None)
            .expect("renders");
        if f.rgba == rgba_of(&fb) {
            break (f.rgba, fb);
        }
        assert!(Instant::now() < end, "never matched the renderer");
    };
    assert_eq!(mine, rgba_of(&expected));
    let distinct: std::collections::HashSet<u32> = expected.pixels().iter().copied().collect();
    assert!(
        distinct.len() > 200,
        "a lit textured picture: {} colours",
        distinct.len()
    );
}

#[test]
fn edit_play_return_edit_again_leaves_the_document_alone_and_leaks_no_thread() {
    let doc = showcase();
    let before = doc.clone();
    let mut studio = Studio::new(&doc);
    assert_eq!(
        studio.enter_play().unwrap_err(),
        "the level is not ready yet"
    );
    assert_eq!(studio.worker_threads(), 0);

    settle(&mut studio, &doc);
    assert!(studio.is_current(&doc));
    assert_eq!(studio.worker_threads(), 1);
    let edit = next_frame(&mut studio, &doc, &[]);

    // Fly: a mouse turn changes the edit picture.
    let turned = loop {
        let f = next_frame(
            &mut studio,
            &doc,
            &[Event::MouseMotion { dx: 150.0, dy: 0.0 }],
        );
        if f.rgba != edit.rgba {
            break f;
        }
    };
    assert_eq!(turned.mode, Mode::Edit);

    for round in 0..2 {
        studio.enter_play().expect("plays");
        let start = next_frame(&mut studio, &doc, &[]);
        assert_eq!(start.mode, Mode::Play, "round {round}");
        let moved = loop {
            let f = next_frame(
                &mut studio,
                &doc,
                &[
                    key(Key::Char('W'), true),
                    Event::MouseMotion { dx: 20.0, dy: 0.0 },
                ],
            );
            if f.rgba != start.rgba {
                break f;
            }
        };
        assert_eq!(moved.mode, Mode::Play);
        studio.leave_play();
        let back = next_frame(&mut studio, &doc, &[]);
        assert_eq!(back.mode, Mode::Edit, "the edit view comes back");
        assert_eq!(doc, before, "play never touches the document");
        assert_eq!(
            studio.worker_threads(),
            1,
            "one thread across play and back"
        );
    }

    // Edit again: Play waits for the new picture, on the same thread.
    let mut state = crate::model::EditorState::new(doc);
    state
        .add_box([40.0, 40.0, 0.0], [44.0, 44.0, 3.0])
        .expect("added");
    assert!(!studio.is_current(&state.doc));
    studio.play_when_ready(&state.doc).expect("will play");
    let end = Instant::now() + Duration::from_secs(60);
    while !studio.tick(&[], PANE, &state.doc).play_now {
        assert!(Instant::now() < end, "Play never started");
        std::thread::sleep(Duration::from_millis(5));
    }
    studio.enter_play().expect("plays again");
    assert_eq!(next_frame(&mut studio, &state.doc, &[]).mode, Mode::Play);
    assert_eq!(studio.worker_threads(), 1);
    studio.reset(&state.doc);
    let end = Instant::now() + Duration::from_secs(10);
    while studio.worker_threads() != 0 {
        studio.tick(&[], PANE, &state.doc);
        assert!(Instant::now() < end, "reset left a render thread running");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!studio.is_current(&state.doc));
}

#[test]
fn an_empty_level_is_refused_for_play_and_leaves_the_studio_usable() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let err = studio.play_when_ready(&Document::new()).unwrap_err();
    assert!(err.contains("no brushes"), "{err}");
    settle(&mut studio, &doc);
    assert!(studio.is_current(&doc));
}

#[test]
fn frame_size_keeps_the_aspect_and_caps_the_width() {
    assert_eq!(frame_size((438, 372), 480), (438, 372));
    assert_eq!(frame_size((1000, 600), 480), (333, 200));
    assert_eq!(frame_size((960, 600), 480), (480, 300));
    assert_eq!(frame_size((0, 0), 480), (16, 16));
}

/// A scene that draws its frame number as the colour, with injectable delay,
/// panic and quit, so tests can tell which frame they were shown.
#[derive(Default)]
struct Probe {
    frames: Arc<AtomicU32>,
    delay: Duration,
    panic_at: Option<u32>,
    quit_after_steps: Option<u32>,
    steps: u32,
    fb: Option<Framebuffer>,
}

impl Scene for Probe {
    fn restart(&mut self) {
        self.steps = 0;
    }

    fn step(&mut self, _: Duration, _: &[Event], _: miniscent_game::FrameStats) -> bool {
        self.steps += 1;
        self.quit_after_steps.is_some_and(|n| self.steps >= n)
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        std::thread::sleep(self.delay);
        let n = self.frames.fetch_add(1, Ordering::SeqCst) + 1;
        if self.panic_at == Some(n) {
            panic!("scene exploded at frame {n}");
        }
        let mut fb = Framebuffer::new(req.w, req.h)?;
        fb.clear(0xff00_0000 | n);
        self.fb = Some(fb);
        self.fb.as_ref()
    }
}

fn frame_number(f: &Frame) -> u32 {
    u32::from(f.rgba[0]) << 16 | u32::from(f.rgba[1]) << 8 | u32::from(f.rgba[2])
}

#[test]
fn a_100_ms_scene_does_not_slow_the_ui_ticks_and_the_newest_frame_is_shown() {
    let doc = showcase();
    let frames = Arc::new(AtomicU32::new(0));
    let mut studio = Studio::new(&doc);
    studio.attach_scene(
        Box::new(Probe {
            frames: frames.clone(),
            delay: Duration::from_millis(100),
            ..Probe::default()
        }),
        &doc,
    );
    studio.enter_play().expect("plays");
    let (mut worst, mut total) = (Duration::ZERO, Duration::ZERO);
    let mut shown = Vec::new();
    let t0 = Instant::now();
    for i in 0..200u32 {
        let events = [key(Key::Char('W'), i % 2 == 0)];
        let t = Instant::now();
        let tick = studio.tick(&events, PANE, &doc);
        let spent = t.elapsed();
        worst = worst.max(spent);
        total += spent;
        if let Some(f) = tick.frame {
            // Only ever shown after it was made, and never an older one.
            assert!(frame_number(&f) <= frames.load(Ordering::SeqCst));
            shown.push(frame_number(&f));
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    let wall = t0.elapsed();
    println!(
        "200 UI ticks over {wall:?}: worst tick {worst:?}, mean {:?}; scene made {} frames, UI showed {}",
        total / 200,
        frames.load(Ordering::SeqCst),
        shown.len()
    );
    assert!(
        worst < Duration::from_millis(5),
        "a UI tick took {worst:?} while the scene needs 100 ms per frame"
    );
    assert!(
        shown.windows(2).all(|w| w[0] < w[1]),
        "frames only move forward: {shown:?}"
    );
    assert!(
        shown.len() >= 2 && (shown.len() as u32) < frames.load(Ordering::SeqCst) + 1,
        "slow frames are shown as they finish: {shown:?}"
    );
    // After the run the newest finished frame is what the UI ends up showing.
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let before = frames.load(Ordering::SeqCst);
        let tick = studio.tick(&[], PANE, &doc);
        if tick.frame.is_some_and(|f| frame_number(&f) + 1 >= before) {
            break;
        }
        assert!(Instant::now() < end, "the UI never caught up");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn a_hidden_pane_is_not_rendered() {
    let doc = showcase();
    let frames = Arc::new(AtomicU32::new(0));
    let mut studio = Studio::new(&doc);
    studio.attach_scene(
        Box::new(Probe {
            frames: frames.clone(),
            ..Probe::default()
        }),
        &doc,
    );
    for _ in 0..30 {
        assert!(studio.tick(&[], (0, 0), &doc).frame.is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        frames.load(Ordering::SeqCst),
        0,
        "nothing drawn at size zero"
    );
    assert_eq!(next_frame(&mut studio, &doc, &[]).w, PANE.0);
}

#[test]
fn a_panicking_scene_is_reported_and_the_editor_stays_up() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    studio.attach_scene(
        Box::new(Probe {
            panic_at: Some(3),
            ..Probe::default()
        }),
        &doc,
    );
    studio.enter_play().expect("plays");
    let end = Instant::now() + Duration::from_secs(10);
    let tick = loop {
        let t = studio.tick(&[], PANE, &doc);
        if t.left_play {
            break t;
        }
        assert!(Instant::now() < end, "the crash was never reported");
        std::thread::sleep(Duration::from_millis(5));
    };
    let status = tick.status.expect("status");
    assert!(
        status.contains("Render thread failed") && status.contains("scene exploded"),
        "{status}"
    );
    assert_eq!(studio.mode, Mode::Edit);
    assert!(!studio.is_current(&doc));
    assert_eq!(
        studio.enter_play().unwrap_err(),
        "the level is not ready yet"
    );
}

#[test]
fn the_game_quitting_returns_to_edit_and_play_restarts() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    studio.attach_scene(
        Box::new(Probe {
            quit_after_steps: Some(5),
            ..Probe::default()
        }),
        &doc,
    );
    for _ in 0..2 {
        studio.enter_play().expect("plays");
        let end = Instant::now() + Duration::from_secs(10);
        while !studio.tick(&[], PANE, &doc).left_play {
            assert!(Instant::now() < end, "quit never arrived");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(studio.mode, Mode::Edit);
    }
}
