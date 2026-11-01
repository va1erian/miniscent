//! The edit, build, play, return, edit loop, driven without a window.

use std::time::Duration;

use mscent_platform::{Event, Key};
use mscent_render::{BuildConfig, LevelRenderer, MaterialTable, Quality};

use super::*;

const DT: Duration = Duration::from_micros(8333);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

fn build_and_wait(studio: &mut Studio, doc: &Document, then_play: bool) -> Tick {
    studio.start_build(doc, then_play).expect("starts");
    loop {
        let t = studio.tick(Duration::ZERO, &[]);
        if !studio.building() {
            return t;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn key(c: Key, pressed: bool) -> Event {
    Event::Key { code: c, pressed }
}

#[test]
fn editor_view_is_pixel_identical_to_the_renderer_for_the_same_camera() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    build_and_wait(&mut studio, &doc, false);
    studio.fly.pos = mscent_math::Vec3::new(2.0, -3.0, 2.0);
    studio.fly.yaw = 0.7;
    studio.fly.pitch = -0.2;
    studio.fly.roll = 0.25;
    let (w, h) = (160, 100);
    let mine = studio
        .render(w, h, &doc)
        .expect("renders")
        .pixels()
        .to_vec();

    let level = doc.to_level().expect("valid");
    let materials = miniscent_game::assets::load_materials(None).expect("textures");
    let direct =
        LevelRenderer::from_level(&level, materials, &BuildConfig::default()).expect("builds");
    let mut fb = mscent_render::Framebuffer::new(w, h).expect("fb");
    let actors = direct.actors().to_vec();
    direct
        .render(&mut fb, &studio.fly.view(), Quality::High, &actors, None)
        .expect("renders");
    assert_eq!(
        mine,
        fb.pixels(),
        "editor and game renderer agree pixel for pixel"
    );
    let _: Option<MaterialTable> = None;
    let distinct: std::collections::HashSet<u32> = mine.iter().copied().collect();
    assert!(
        distinct.len() > 200,
        "a lit textured picture: {} colours",
        distinct.len()
    );
}

#[test]
fn edit_play_return_edit_again_leaves_the_document_alone() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    assert!(
        studio.render(64, 40, &doc).is_none(),
        "nothing before a build"
    );
    assert_eq!(studio.enter_play().unwrap_err(), "build the level first");

    let t = build_and_wait(&mut studio, &doc, true);
    assert!(t.play_now && !t.log.is_empty(), "{t:?}");
    assert!(!studio.stale(&doc));
    studio.enter_play().expect("plays");
    assert_eq!(studio.mode, Mode::Play);

    let before = doc.clone();
    let start = studio
        .render(120, 75, &doc)
        .expect("play frame")
        .pixels()
        .to_vec();
    let mut frames = vec![key(Key::Char('W'), true)];
    for i in 0..120 {
        studio.tick(DT, &frames);
        frames.clear();
        if i == 60 {
            frames.push(Event::MouseMotion { dx: 20.0, dy: 0.0 });
        }
    }
    let later = studio
        .render(120, 75, &doc)
        .expect("play frame")
        .pixels()
        .to_vec();
    assert_ne!(
        start, later,
        "moving forward and turning changed the picture"
    );

    studio.leave_play();
    assert_eq!(studio.mode, Mode::Edit);
    assert_eq!(doc, before, "play never touches the document");

    // Edit again: the build is now stale, rebuilding and playing works.
    let mut state = crate::model::EditorState::new(doc);
    state
        .add_box([40.0, 40.0, 0.0], [44.0, 44.0, 3.0])
        .expect("added");
    assert!(studio.stale(&state.doc));
    let t = build_and_wait(&mut studio, &state.doc, true);
    assert!(t.play_now);
    assert!(!studio.stale(&state.doc));
    studio.enter_play().expect("plays again");
    for _ in 0..30 {
        studio.tick(DT, &[]);
    }
    assert!(studio.render(64, 40, &state.doc).is_some());
}

#[test]
fn failed_build_reports_and_leaves_the_studio_usable() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    let err = studio.start_build(&Document::new(), false).unwrap_err();
    assert!(err.contains("no brushes"), "{err}");
    assert!(!studio.building());
    build_and_wait(&mut studio, &doc, false);
    assert!(studio.has_build());
    // A second request while one runs is refused, not queued.
    studio.start_build(&doc, false).expect("starts");
    assert!(studio.start_build(&doc, false).is_err());
    while studio.building() {
        studio.tick(Duration::ZERO, &[]);
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn frame_size_keeps_the_aspect_and_caps_the_width() {
    assert_eq!(frame_size((438, 372), 480), (438, 372));
    assert_eq!(frame_size((1000, 600), 480), (333, 200));
    assert_eq!(frame_size((960, 600), 480), (480, 300));
    assert_eq!(frame_size((0, 0), 480), (16, 16));
}

#[test]
fn flying_changes_the_picture_and_marks_a_redraw() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    build_and_wait(&mut studio, &doc, false);
    let a = studio
        .render(96, 60, &doc)
        .expect("frame")
        .pixels()
        .to_vec();
    let t = studio.tick(DT, &[Event::MouseMotion { dx: 150.0, dy: 0.0 }]);
    assert!(t.redraw);
    let b = studio
        .render(96, 60, &doc)
        .expect("frame")
        .pixels()
        .to_vec();
    assert_ne!(a, b);
    let quiet = studio.tick(DT, &[]);
    assert!(!quiet.redraw, "an idle frame costs nothing");
}
