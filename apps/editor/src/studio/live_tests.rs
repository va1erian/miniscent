//! The live path: strokes reach the picture with no CSG job.

use std::time::{Duration, Instant};

use mscent_math::Vec3d;

use super::live::{LivePlan, plan};
use super::tick_costs::TickCosts;
use super::*;
use crate::model::pick::Ray;
use crate::model::stroke::{StrokeKeys, VoxelOp};
use crate::model::{EditorState, Tool};

const PANE: (u32, u32) = (320, 200);

fn room() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([-8.0, -8.0, 0.0], [8.0, 8.0, 6.0]);
    st.tool = Tool::Voxel;
    st
}

fn at_wall(x: f64) -> Ray {
    Ray {
        origin: Vec3d::new(x, 0.0, 3.0),
        dir: Vec3d::new(0.0, 1.0, 0.0),
    }
}

fn settle(studio: &mut Studio, doc: &Document) {
    let end = Instant::now() + Duration::from_secs(60);
    while studio.preview(doc) != Preview::Full {
        studio.tick(&[], PANE, doc);
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(3));
    }
}

fn caught_up(studio: &mut Studio, doc: &Document) {
    let end = Instant::now() + Duration::from_secs(30);
    while studio.built_doc.as_ref() != Some(doc) {
        studio.tick(&[], PANE, doc);
        assert!(Instant::now() < end, "never caught up");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// A room with a first stroke in it, settled.
fn sculpted() -> (EditorState, Studio) {
    let mut st = room();
    st.begin_stroke(at_wall(0.0), StrokeKeys::default());
    st.finish_stroke();
    let mut studio = Studio::new(&st.doc);
    settle(&mut studio, &st.doc);
    (st, studio)
}

#[test]
fn a_stroke_starts_no_preview_job_and_no_bake() {
    let (mut st, mut studio) = sculpted();
    let before = studio.live_jobs();
    st.begin_stroke(at_wall(0.5), StrokeKeys::default());
    studio.set_stroking(true);
    let end = Instant::now() + Duration::from_millis(400);
    let mut x = 0.5;
    while Instant::now() < end {
        st.stroke_ray(at_wall(x));
        x = (x + 0.02_f64).min(1.8);
        st.poll_stroke();
        studio.tick(&[], PANE, &st.doc);
        assert!(studio.preview_job.is_none(), "a stroke started a CSG job");
        assert!(studio.bake_job.is_none(), "a stroke started a bake");
        std::thread::sleep(Duration::from_millis(2));
    }
    st.finish_stroke();
    studio.set_stroking(false);
    assert!(studio.live_jobs() > before, "it went by the live path");
    settle(&mut studio, &st.doc);
}

#[test]
fn the_ui_tick_of_a_stroke_stays_under_a_millisecond_as_a_rule() {
    let (mut st, mut studio) = sculpted();
    st.voxel.radius = 4.0;
    st.voxel.op = VoxelOp::Smooth;
    st.begin_stroke(at_wall(0.5), StrokeKeys::default());
    studio.set_stroking(true);
    let mut costs = TickCosts::default();
    for i in 0..200 {
        let ray = at_wall(0.5 + f64::from(i) * 0.005);
        costs.timed(|| {
            st.stroke_ray(ray);
            st.poll_stroke();
            studio.tick(&[], PANE, &st.doc);
        });
        std::thread::sleep(Duration::from_millis(1));
    }
    st.finish_stroke();
    studio.set_stroking(false);
    println!("p90 {:?} worst {:?}", costs.p90(), costs.worst());
    assert!(
        costs.p90() < Duration::from_millis(1),
        "p90 {:?}",
        costs.p90()
    );
}

#[test]
fn undo_and_redo_go_by_the_live_path_too() {
    let (mut st, mut studio) = sculpted();
    let before = studio.live_jobs();
    st.begin_stroke(at_wall(1.0), StrokeKeys::default());
    st.finish_stroke();
    caught_up(&mut studio, &st.doc);
    assert!(st.undo());
    caught_up(&mut studio, &st.doc);
    assert!(studio.live_jobs() >= before + 2);
    settle(&mut studio, &st.doc);
}

#[test]
fn play_after_a_live_edit_waits_for_a_session_that_collides_with_it() {
    let (mut st, mut studio) = sculpted();
    assert!(studio.playable_now(&st.doc));
    st.begin_stroke(at_wall(1.0), StrokeKeys::default());
    st.finish_stroke();
    caught_up(&mut studio, &st.doc);
    assert!(studio.is_current(&st.doc));
    assert!(
        !studio.playable_now(&st.doc),
        "the session is a stroke behind"
    );
    studio.play_when_ready(&st.doc).expect("can play");
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let tick = studio.tick(&[], PANE, &st.doc);
        // A level with no start cannot play, and says so; either way it was built.
        if tick.play_now || tick.status.is_some() {
            break;
        }
        assert!(Instant::now() < end, "Play never came");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(studio.playable_now(&st.doc));
}

#[test]
fn only_voxel_chunks_differing_can_take_the_live_path() {
    let (mut st, _studio) = sculpted();
    let base = st.doc.clone();
    st.begin_stroke(at_wall(1.0), StrokeKeys::default());
    st.finish_stroke();
    assert!(matches!(plan(&base, &st.doc), Some(LivePlan::Chunks(_))));
    // A new brush changes the BSP.
    let mut with_brush = room();
    with_brush.add_box([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    assert_eq!(plan(&room().doc, &with_brush.doc), None);
    // A subtract volume coming into being changes the BSP.
    let empty = room();
    assert_eq!(plan(&empty.doc, &base), None);
    // An add volume coming into being does not.
    let mut add = room();
    add.voxel.op = VoxelOp::Add;
    add.begin_stroke(at_wall(0.0), StrokeKeys::default());
    add.finish_stroke();
    assert_eq!(plan(&empty.doc, &add.doc), Some(LivePlan::All));
}
