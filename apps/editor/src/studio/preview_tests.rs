//! The 3D pane follows the document with no command from anyone: these tests
//! only ever edit the document and call `tick`, as the UI's timer does.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use mscent_math::Vec3;
use mscent_platform::{Event, Key};
use mscent_render::Framebuffer;
use mscent_world::lightmap::MAX_SHADOW_SAMPLES;
use mscent_world::{Entity, Op};

use super::scene::LevelScene;
use super::tick_costs::TickCosts;
use super::worker::Request;
use super::*;
use crate::build::{self, Light};
use crate::model::{EditorState, FlyCam, Selection};

const PANE: (u32, u32) = (480, 300);

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

/// What ticking until a condition held looked like from the UI's side.
#[derive(Default)]
pub(super) struct Watch {
    pub states: HashSet<String>,
    pub log: Vec<String>,
    pub status: Vec<String>,
    pub costs: TickCosts,
    pub took: Duration,
}

/// Ticks until `done`, with nothing but ticks.
pub(super) fn tick_until(
    studio: &mut Studio,
    doc: &Document,
    done: impl Fn(&Studio) -> bool,
) -> Watch {
    tick_within(studio, doc, Duration::from_secs(120), done)
}

pub(super) fn tick_within(
    studio: &mut Studio,
    doc: &Document,
    limit: Duration,
    done: impl Fn(&Studio) -> bool,
) -> Watch {
    let mut w = Watch::default();
    let t0 = Instant::now();
    let end = t0 + limit;
    loop {
        let tick = w.costs.timed(|| studio.tick(&[], PANE, doc));
        w.log.extend(tick.log);
        w.status.extend(tick.status);
        w.states.insert(format!("{:?}", studio.preview(doc)));
        if done(studio) {
            w.took = t0.elapsed();
            return w;
        }
        assert!(Instant::now() < end, "never settled: {:?}", w.states);
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn settle(studio: &mut Studio, doc: &Document) -> Watch {
    tick_until(studio, doc, |s| s.preview(doc) == Preview::Full)
}

pub(super) fn bakes(studio: &Studio) -> u32 {
    studio.cache().lock().expect("cache").bakes()
}

/// A settled studio on the showcase.
pub(super) fn settled() -> (Studio, Document) {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    settle(&mut studio, &doc);
    (studio, doc)
}

/// `doc` with one more solid box at `x`, far from everything else.
pub(super) fn with_box(doc: &Document, x: f64) -> Document {
    let mut st = EditorState::new(doc.clone());
    st.new_op = Op::Add;
    st.add_box([x, 40.0, 0.0], [x + 2.0, 42.0, 2.0])
        .expect("added");
    st.doc
}

/// Frames up to the first one that differs from `before`; the UI never waits,
/// the test does.
fn frame_other_than(studio: &mut Studio, doc: &Document, before: &[u8]) -> Frame {
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let tick = studio.tick(&[], PANE, doc);
        if let Some(f) = tick.frame
            && f.rgba != before
        {
            return f;
        }
        assert!(Instant::now() < end, "the picture never changed");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn any_frame(studio: &mut Studio, doc: &Document) -> Frame {
    frame_other_than(studio, doc, &[])
}

#[test]
fn the_first_picture_needs_no_command_and_the_lighting_finishes_by_itself() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    assert_eq!(studio.preview(&doc), Preview::Updating);
    let w = settle(&mut studio, &doc);
    assert!(w.states.contains("Baking"), "a draft first: {:?}", w.states);
    assert_eq!(bakes(&studio), 1);
    assert!(
        w.log.iter().any(|l| l.starts_with("Lightmap baked in")),
        "{:?}",
        w.log
    );
    assert!(any_frame(&mut studio, &doc).w > 0);
}

#[test]
fn an_edit_shows_up_on_draft_light_at_once_and_the_ui_never_waits_for_the_bake() {
    let (mut studio, doc) = settled();
    // Look straight at where the new box will stand.
    studio.set_camera(FlyCam::new(Vec3::new(41.0, 33.0, 1.0), 0.0, 0.0));
    let before = any_frame(&mut studio, &doc);
    let edited = with_box(&doc, 40.0);
    let shown = tick_until(&mut studio, &edited, |s| s.is_current(&edited));
    let draft = studio.light;
    let after = frame_other_than(&mut studio, &edited, &before.rgba);
    assert_ne!(after.rgba, before.rgba, "the new box is on the picture");
    let lit = settle(&mut studio, &edited);
    let costs = shown.costs.merged(&lit.costs);
    println!(
        "edit to picture {:?}, then to full light {:?}; UI ticks p90 {:?}, worst {:?}",
        shown.took,
        lit.took,
        costs.p90(),
        costs.worst()
    );
    costs.assert_cheap();
    assert_eq!(draft, Light::Draft, "the box needs new light");
    assert_eq!(bakes(&studio), 2, "the baker ran for it, unasked");
}

#[test]
fn a_drag_runs_no_bake_and_ends_on_its_last_position() {
    let (mut studio, doc) = settled();
    let mut st = EditorState::new(doc.clone());
    st.new_op = Op::Add;
    st.add_box([40.0, 40.0, 0.0], [42.0, 42.0, 2.0])
        .expect("added");
    let id = st.doc.brushes().last().expect("brush").id;
    let original = st.doc.brush(id).expect("alive").clone();
    let mut costs = TickCosts::default();
    for i in 0..60 {
        let mut moved = original.clone();
        moved.translate([f64::from(i) * 0.25, 0.0, 0.0]);
        st.doc.replace_brush_for_test(&moved);
        costs.timed(|| studio.tick(&[], PANE, &st.doc));
        assert!(
            studio.bake_job.is_none(),
            "no bake while the level is moving"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(bakes(&studio), 1, "only the one from before the drag");
    costs.assert_cheap();
    settle(&mut studio, &st.doc);
    assert!(studio.is_current(&st.doc), "the last position won");
    assert_eq!(bakes(&studio), 2, "one bake for the whole drag");
}

#[test]
fn a_texture_edit_keeps_the_full_light_and_never_shows_a_draft() {
    let (mut studio, doc) = settled();
    let mut st = EditorState::new(doc.clone());
    st.selection = Some(Selection::Brush(doc.brushes()[0].id));
    assert!(st.apply_face_edit(crate::model::facetex::FaceEdit::Rotation(45.0)));
    let w = settle(&mut studio, &st.doc);
    assert!(!w.states.contains("Baking"), "{:?}", w.states);
    assert!(w.log.is_empty(), "no news in the log: {:?}", w.log);
    assert_eq!(bakes(&studio), 1);
}

#[test]
fn moving_an_enemy_needs_no_new_light_but_moving_a_lamp_does() {
    let (mut studio, doc) = settled();
    let moved = |kind: fn(&Entity) -> bool| {
        let mut d = doc.clone();
        let mut def = *d
            .entities()
            .iter()
            .find(|e| kind(&e.entity))
            .expect("found");
        def.entity = crate::model::entity::with_pos(&def.entity, [5.0, 5.0, 2.0]);
        d.replace_entity_for_test(&def);
        d
    };
    let enemy = moved(|e| matches!(e, Entity::Enemy { .. }));
    let w = settle(&mut studio, &enemy);
    assert!(!w.states.contains("Baking"), "{:?}", w.states);
    assert_eq!(bakes(&studio), 1);
    let lamp = moved(|e| matches!(e, Entity::Light { .. }));
    settle(&mut studio, &lamp);
    assert_eq!(bakes(&studio), 2);
}

#[test]
fn undoing_an_edit_before_it_was_baked_needs_no_bake() {
    let (mut studio, doc) = settled();
    let edited = with_box(&doc, 40.0);
    tick_until(&mut studio, &edited, |s| s.is_current(&edited));
    let w = settle(&mut studio, &doc);
    assert!(w.states.contains("Full"));
    assert!(bakes(&studio) <= 2, "at most the edit's own bake");
}

#[test]
fn an_edit_during_a_bake_drops_it_and_the_newest_level_gets_the_light() {
    let (mut studio, doc) = settled();
    let first = with_box(&doc, 40.0);
    tick_until(&mut studio, &first, |s| s.bake_job.is_some());
    let second = with_box(&first, 50.0);
    let w = settle(&mut studio, &second);
    w.costs.assert_cheap();
    assert!(studio.is_current(&second));
    assert!(bakes(&studio) <= 3, "{} bakes", bakes(&studio));
    // The picture's lightmap is the second level's, not the dropped one's.
    let key = studio.light_key;
    let cache = studio.cache();
    let job = build::start_preview(&second, &cache, &SessionConfig::default()).expect("starts");
    let built = loop {
        if let Some(r) = job.poll() {
            break r.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(built.light, Light::Full);
    assert_eq!(built.light_key, key);
}

#[test]
fn a_bake_that_fails_keeps_the_draft_says_so_and_is_not_retried() {
    // The baker lays any real level out by coarsening, so the bake is made to
    // fail on settings only it reads: a draft always traces one ray.
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    studio.settings.build.shadow_samples = MAX_SHADOW_SAMPLES + 1;
    let shown = tick_until(&mut studio, &doc, |s| s.is_current(&doc));
    println!(
        "first picture {:?}, worst UI tick {:?}",
        shown.took,
        shown.costs.worst()
    );
    let w = tick_until(&mut studio, &doc, |s| s.preview(&doc) == Preview::DraftOnly);
    assert!(
        w.status.iter().any(|s| s.starts_with("Keeping draft")),
        "{:?}",
        w.status
    );
    assert_eq!(w.log.iter().filter(|l| l.starts_with("ERROR")).count(), 1);
    assert!(studio.is_current(&doc), "the level is on screen anyway");
    assert!(any_frame(&mut studio, &doc).w > 0);
    for _ in 0..50 {
        let t = studio.tick(&[], PANE, &doc);
        assert!(t.log.is_empty(), "retried: {:?}", t.log);
        assert!(studio.bake_job.is_none());
        std::thread::sleep(Duration::from_millis(5));
    }
    studio.enter_play().expect("draft light is enough to play");
}

#[test]
fn an_empty_level_takes_the_picture_away_and_a_new_brush_brings_one_back() {
    let (mut studio, doc) = settled();
    studio.set_camera(FlyCam::new(Vec3::new(0.0, 0.0, 1.0), 0.5, 0.0));
    // The user has flown there: the render thread owns the camera now.
    tick_until(&mut studio, &doc, |s| s.fly_camera().yaw == 0.5);
    let empty = Document::new();
    let tick = studio.tick(&[], PANE, &empty);
    assert!(tick.blank.expect("blank").contains("empty"));
    assert_eq!(studio.preview(&empty), Preview::Empty);
    assert!(studio.tick(&[], PANE, &empty).blank.is_none(), "only once");
    assert!(
        studio
            .play_when_ready(&empty)
            .unwrap_err()
            .contains("no brushes")
    );
    assert!(studio.enter_play().is_err());
    let end = Instant::now() + Duration::from_secs(10);
    while studio.worker_threads() != 0 {
        studio.tick(&[], PANE, &empty);
        assert!(Instant::now() < end, "the render thread never ended");
        std::thread::sleep(Duration::from_millis(5));
    }

    settle(&mut studio, &doc);
    assert!(any_frame(&mut studio, &doc).w > 0);
    assert_eq!(studio.fly_camera().yaw, 0.5, "the camera stayed put");
}

/// `doc` with its last brush made invalid, which the model itself refuses.
fn corrupted(doc: &Document) -> Document {
    let mut bad = doc.clone();
    let mut def = bad.brushes().last().expect("brush").clone();
    def.shape = crate::model::document::Shape::Box {
        min: [1.0, 1.0, 1.0],
        max: [1.0, 1.0, 1.0],
    };
    bad.replace_brush_for_test(&def);
    bad
}

#[test]
fn a_level_that_cannot_compile_keeps_the_last_picture_and_says_why_once() {
    let (mut studio, doc) = settled();
    let bad = corrupted(&doc);
    let w = tick_until(&mut studio, &bad, |s| s.preview(&bad) == Preview::Failed);
    assert!(
        w.status.iter().any(|s| s.contains("is invalid")),
        "{:?}",
        w.status
    );
    assert_eq!(w.log.len(), 1, "{:?}", w.log);
    assert_eq!(studio.worker_threads(), 1, "the old picture stays up");
    assert!(
        studio
            .play_when_ready(&bad)
            .unwrap_err()
            .contains("is invalid")
    );
    for _ in 0..20 {
        let t = studio.tick(&[], PANE, &bad);
        assert!(t.log.is_empty() && t.status.is_none(), "not tried again");
        std::thread::sleep(Duration::from_millis(2));
    }
    settle(&mut studio, &doc);
    assert_eq!(studio.preview(&doc), Preview::Full, "fixing it recovers");
}

#[test]
fn a_level_that_never_could_be_shown_says_why_in_the_pane() {
    let bad = corrupted(&showcase());
    let mut studio = Studio::new(&bad);
    let end = Instant::now() + Duration::from_secs(60);
    let text = loop {
        if let Some(t) = studio.tick(&[], PANE, &bad).blank {
            break t;
        }
        assert!(Instant::now() < end, "never reported");
        std::thread::sleep(Duration::from_millis(2));
    };
    assert!(text.contains("is invalid"), "{text}");
}

#[test]
fn play_waits_for_the_preview_not_for_the_bake_and_the_light_arrives_during_the_game() {
    let (mut studio, doc) = settled();
    let edited = with_box(&doc, 40.0);
    assert!(!studio.is_current(&edited));
    studio.play_when_ready(&edited).expect("will play");
    let end = Instant::now() + Duration::from_secs(60);
    while !studio.tick(&[], PANE, &edited).play_now {
        assert!(Instant::now() < end, "Play never started");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(studio.light, Light::Draft, "it did not wait for the bake");
    studio.enter_play().expect("plays");

    let w = settle(&mut studio, &edited);
    assert!(w.states.contains("Baking"));
    assert_eq!(studio.mode, Mode::Play, "the game goes on through the swap");
    assert_eq!(any_frame(&mut studio, &edited).mode, Mode::Play);
    assert_eq!(bakes(&studio), 2);
}

#[test]
fn new_light_replaces_the_renderer_under_a_running_game() {
    let doc = showcase();
    let cache = SharedCache::default();
    let wait = |job: build::Job<build::Built>| loop {
        if let Some(r) = job.poll() {
            return r.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let draft =
        wait(build::start_preview(&doc, &cache, &SessionConfig::default()).expect("starts"));
    let key = draft.light_key;
    let area = |b: &build::Built| b.renderer().lightmap().atlas().height();
    let draft_height = area(&draft);
    let bake = build::start_bake(&doc, key, &cache, &SessionConfig::default()).expect("starts");
    while bake.job.poll().is_none() {
        std::thread::sleep(Duration::from_millis(5));
    }
    let full = wait(build::start_preview(&doc, &cache, &SessionConfig::default()).expect("starts"));
    assert!(area(&full) > draft_height);

    let mut scene = LevelScene::new(draft);
    let press = [Event::Key {
        code: Key::Char('W'),
        pressed: true,
    }];
    for _ in 0..60 {
        scene.step(Duration::from_millis(16), &press, Default::default());
    }
    let pos = |s: &mut LevelScene| s.session().expect("playable").game().player.pos();
    let moved = pos(&mut scene);
    assert_ne!(moved, mscent_math::Vec3::ZERO);
    scene.relight(full);
    assert_eq!(pos(&mut scene), moved, "the game state survived");
    let height = scene.session().expect("playable").renderer();
    assert!(height.lightmap().atlas().height() > draft_height);
}

/// Draws one colour and counts the cameras it was shown.
#[derive(Default)]
struct Flat {
    fb: Option<Framebuffer>,
}

impl Scene for Flat {
    fn restart(&mut self) {}

    fn step(&mut self, _: Duration, _: &[Event], _: miniscent_game::FrameStats) -> bool {
        false
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        self.fb = Framebuffer::new(req.w, req.h);
        self.fb.as_ref()
    }
}

#[test]
fn a_new_scene_keeps_the_thread_the_camera_and_a_held_key() {
    let doc = showcase();
    let mut studio = Studio::new(&doc);
    studio.attach_scene(Box::<Flat>::default(), &doc);
    let w = [Event::Key {
        code: Key::Char('W'),
        pressed: true,
    }];
    studio.tick(&w, PANE, &doc);
    let start = studio.fly_camera().pos;
    let end = Instant::now() + Duration::from_secs(10);
    while studio.fly_camera().pos == start {
        studio.tick(&[], PANE, &doc);
        assert!(Instant::now() < end, "W did not move the camera");
        std::thread::sleep(Duration::from_millis(5));
    }
    studio.attach_scene(Box::<Flat>::default(), &doc);
    studio.tick(&[], PANE, &doc);
    std::thread::sleep(Duration::from_millis(50));
    let swapped = studio.fly_camera().pos;
    assert_ne!(swapped, start, "the camera was not reset");
    let end = Instant::now() + Duration::from_secs(10);
    while studio.fly_camera().pos == swapped {
        studio.tick(&[], PANE, &doc);
        assert!(Instant::now() < end, "the held W was lost in the swap");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(studio.worker_threads(), 1, "the same thread");
}
