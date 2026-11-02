//! The tools beyond Add and Carve, driven the way the 3D view drives them:
//! Flatten, Roughen, Tunnel, Copy and Stamp, generators and Fill.

use mscent_math::Vec3d;
use mscent_world::voxel::{Context, Volume, first_solid};

use super::generators::Kind;
use super::pick::Ray;
use super::state::{EditorState, Selection};
use super::stroke::{StrokeKeys, VoxelOp};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// A room with its +Y wall at y = 8 and its floor at z = 0.
fn room_state() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([-8.0, -8.0, 0.0], [8.0, 8.0, 6.0]);
    st
}

fn at_wall(x: f64) -> Ray {
    Ray {
        origin: v(x, 0.0, 3.0),
        dir: v(0.0, 1.0, 0.0),
    }
}

fn on_floor(x: f64, y: f64) -> Ray {
    Ray {
        origin: v(x, y, 3.0),
        dir: v(0.0, 0.0, -1.0),
    }
}

fn stroke(st: &mut EditorState, op: VoxelOp, rays: &[Ray]) {
    st.voxel.op = op;
    assert!(st.begin_stroke(rays[0], StrokeKeys::default()));
    for r in &rays[1..] {
        st.stroke_ray(*r);
    }
    st.finish_stroke();
}

fn volumes(st: &EditorState) -> Vec<Volume> {
    st.doc.volumes().map(|(_, v)| v.clone()).collect()
}

/// How far a ray from the middle of the room goes before it meets the wall or
/// the volume's surface.
fn depth(st: &EditorState, x: f64) -> f64 {
    let level = st.doc.to_level().expect("exports");
    let brushes = level.static_brushes();
    let (_, volume) = st.doc.volumes().next().expect("a volume");
    let mut ctx = Context::new(volume, &brushes);
    first_solid(volume, &mut ctx, v(x, 0.0, 3.0), v(0.0, 1.0, 0.0), 100.0).expect("hits")
}

#[test]
fn flatten_levels_a_pit_back_to_the_plane_of_the_first_hit() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Carve, &[at_wall(0.0)]);
    let pit = depth(&st, 0.0);
    assert!(pit > 8.5, "the pit is dug: {pit}");
    st.voxel.radius = 1.5;
    let before = st.history.depth();
    stroke(
        &mut st,
        VoxelOp::Flatten,
        &[at_wall(-1.4), at_wall(0.0), at_wall(1.4)],
    );
    let level = depth(&st, 0.0);
    assert!((level - 8.0).abs() < 0.3, "the wall is back: {level}");
    assert_eq!(st.history.depth(), before + 1, "one undo step");
}

#[test]
fn roughen_with_the_same_seed_bumps_the_same_way() {
    let rough = |seed: u32| {
        let mut st = room_state();
        stroke(&mut st, VoxelOp::Carve, &[at_wall(0.0)]);
        let smooth = volumes(&st);
        st.voxel.seed = seed;
        stroke(&mut st, VoxelOp::Roughen, &[at_wall(-0.5), at_wall(0.5)]);
        (smooth, volumes(&st))
    };
    let (smooth, a) = rough(3);
    assert_ne!(smooth, a, "roughening changed the surface");
    assert_eq!(a, rough(3).1);
    assert_ne!(a, rough(4).1);
}

#[test]
fn tunnel_clicks_dig_on_from_each_other_and_escape_ends_the_path() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Tunnel, &[at_wall(0.0)]);
    let mouth = depth(&st, 0.0);
    let first = st.voxel.path.expect("the path starts");
    assert!(
        (first.0.y - 8.0).abs() < 0.1,
        "the first point is on the wall"
    );
    stroke(&mut st, VoxelOp::Tunnel, &[at_wall(0.0)]);
    let second = st.voxel.path.expect("the path goes on");
    assert!(second.0.y > first.0.y, "each click digs on");
    assert!(depth(&st, 0.0) > mouth + 0.5, "the tunnel is deeper");
    st.end_path();
    assert!(st.voxel.path.is_none());
    // One undo step per click.
    assert!(st.undo() && st.undo());
    assert!(st.doc.volumes().next().is_none());
}

#[test]
fn copy_captures_a_ball_and_stamp_pastes_it_in_one_step() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Stamp, &[at_wall(3.0)]);
    assert!(st.doc.volumes().next().is_none(), "nothing copied yet");
    assert!(st.message.contains("Copy"), "{}", st.message);
    stroke(&mut st, VoxelOp::Carve, &[at_wall(-3.0)]);
    st.voxel.radius = 1.5;
    stroke(&mut st, VoxelOp::Copy, &[at_wall(-3.0)]);
    assert!(st.clip.is_some(), "{}", st.message);
    let steps = st.history.depth();
    stroke(&mut st, VoxelOp::Stamp, &[on_floor(3.0, 0.0)]);
    assert_eq!(st.doc.volumes().count(), 2, "the stamp made a volume");
    assert_eq!(st.history.depth(), steps + 1);
    assert!(st.clip.is_some(), "the clip stays for another stamp");
}

#[test]
fn copy_with_no_volume_under_it_says_so_and_makes_nothing() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Copy, &[at_wall(0.0)]);
    assert!(st.clip.is_none());
    assert!(st.doc.volumes().next().is_none());
    assert!(st.message.contains("no volume"), "{}", st.message);
}

#[test]
fn a_generator_is_held_until_commit_and_rolls_by_seed() {
    let mut st = room_state();
    st.voxel.generator = Some(Kind::Mushrooms);
    st.voxel.seed = 5;
    let steps = st.history.depth();
    assert!(st.begin_stroke(on_floor(0.0, 0.0), StrokeKeys::default()));
    st.finish_stroke();
    assert!(st.pending(), "{}", st.message);
    assert_eq!(
        st.history.depth(),
        steps,
        "nothing is recorded before Commit"
    );
    let first = volumes(&st);
    assert_eq!(first.len(), 1);

    assert!(st.reroll_to(6));
    st.finish_stroke();
    let second = volumes(&st);
    assert_eq!(second.len(), 1);
    assert_ne!(first, second, "another seed grows another result");
    assert!(st.reroll_to(5));
    st.finish_stroke();
    assert_eq!(volumes(&st), first, "the same seed grows the same result");

    assert!(st.commit_pending());
    assert!(!st.pending());
    assert_eq!(st.history.depth(), steps + 1);
    assert!(st.undo());
    assert!(st.doc.volumes().next().is_none());
}

#[test]
fn escape_drops_a_held_generation() {
    let mut st = room_state();
    st.voxel.generator = Some(Kind::Coral);
    let steps = st.history.depth();
    assert!(st.begin_stroke(on_floor(0.0, 0.0), StrokeKeys::default()));
    st.finish_stroke();
    assert!(st.pending() && st.doc.volumes().count() == 1);
    st.cancel_drag();
    assert!(!st.pending());
    assert!(st.doc.volumes().next().is_none());
    assert_eq!(st.history.depth(), steps);
    assert!(!st.commit_pending(), "nothing to commit");
}

#[test]
fn fill_pours_a_field_into_the_selected_volume_as_one_undo_step() {
    let mut st = room_state();
    assert!(!st.fill_selected(), "a box is not a volume");
    stroke(&mut st, VoxelOp::Carve, &[at_wall(0.0)]);
    let (id, _) = st
        .doc
        .volumes()
        .next()
        .map(|(d, v)| (d.id, v.clone()))
        .expect("a volume");
    let carved = volumes(&st);
    st.selection = Some(Selection::Brush(id));
    let steps = st.history.depth();
    assert!(st.fill_selected());
    st.finish_stroke();
    let filled = volumes(&st);
    assert_ne!(carved, filled, "the field replaced the sculpt");
    assert_eq!(st.history.depth(), steps + 1);
    assert!(st.undo());
    assert_eq!(volumes(&st), carved);
}

#[test]
fn undo_with_a_held_result_only_takes_that_away() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Carve, &[at_wall(0.0)]);
    let depth = st.history.depth();
    let carved = volumes(&st);
    st.voxel.generator = Some(Kind::Mushrooms);
    assert!(st.begin_stroke(on_floor(-5.0, -5.0), StrokeKeys::default()));
    st.finish_stroke();
    assert!(st.pending());
    assert!(st.undo());
    assert!(!st.pending());
    assert_eq!(st.history.depth(), depth, "the carve is still a step");
    assert_eq!(volumes(&st), carved, "and still there");
}

#[test]
fn a_generation_that_makes_nothing_holds_nothing_and_says_why() {
    let mut st = room_state();
    st.voxel.generator = Some(Kind::Mushrooms);
    let away = Ray {
        origin: mscent_math::Vec3d::new(0.0, 0.0, 3.0),
        dir: mscent_math::Vec3d::new(0.0, 0.0, 0.0),
    };
    assert!(st.begin_stroke(away, StrokeKeys::default()));
    st.finish_stroke();
    assert!(!st.pending());
    assert!(st.doc.volumes().next().is_none());
    assert!(!st.message.contains("Commit"), "{}", st.message);
}

#[test]
fn undoing_a_tunnel_click_forgets_where_it_got_to() {
    let mut st = room_state();
    stroke(&mut st, VoxelOp::Tunnel, &[at_wall(0.0)]);
    stroke(&mut st, VoxelOp::Tunnel, &[at_wall(1.0)]);
    assert!(st.voxel.path.is_some());
    assert!(st.undo());
    assert!(st.voxel.path.is_none());
}

#[test]
fn working_on_in_a_view_keeps_what_was_grown() {
    use crate::model::{Pointer, View};
    let mut st = room_state();
    st.voxel.generator = Some(Kind::Mushrooms);
    let depth = st.history.depth();
    assert!(st.begin_stroke(on_floor(0.0, 0.0), StrokeKeys::default()));
    st.finish_stroke();
    assert!(st.pending());
    st.pointer_down(Pointer {
        view: View::Top,
        pos: [20.0, 20.0],
        px: 0.1,
    });
    assert!(!st.pending());
    assert_eq!(st.history.depth(), depth + 1);
}
