//! Volumes in the document and strokes, driven the way the 3D view drives them.

use mscent_math::Vec3d;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab};
use mscent_world::{Brush, Level, Op, compile, format};

use super::document::{Document, Shape};
use super::pick::Ray;
use super::state::EditorState;
use super::stroke::{StrokeKeys, VoxelOp};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// A room with its +Y wall at y = 8.
fn room_state() -> EditorState {
    let mut st = EditorState::default();
    st.add_box([-8.0, -8.0, 0.0], [8.0, 8.0, 6.0]);
    st
}

/// Looking at the wall ahead from the middle of the room, `x` along it.
fn at_wall(x: f64) -> Ray {
    Ray {
        origin: v(x, 0.0, 3.0),
        dir: v(0.0, 1.0, 0.0),
    }
}

fn the_volume(st: &EditorState) -> Volume {
    let (_, v) = st.doc.volumes().next().expect("a volume");
    v.clone()
}

/// Triangles air can see in the level as it now is.
fn surface(st: &EditorState) -> usize {
    let level = st.doc.to_level().expect("exports");
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    world.chunks.iter().map(|c| c.lods[0].triangles.len()).sum()
}

fn stroke(st: &mut EditorState, rays: &[Ray]) {
    assert!(st.begin_stroke(rays[0], StrokeKeys::default()));
    for r in &rays[1..] {
        st.stroke_ray(*r);
    }
    st.finish_stroke();
}

#[test]
fn volumes_survive_loading_into_a_document_and_saving_again() {
    let mut level = mscent_world::showcase_level();
    let after = level.brushes.iter().map(Brush::order).max().unwrap_or(0) + 1;
    let volume = Volume::new(Op::Add, after, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    level.volumes.push(volume);
    let bytes = format::write(&level);
    let doc = Document::from_level(&format::read(&bytes).expect("reads"));
    let saved = doc.to_level().expect("exports");
    assert_eq!(saved.volumes.len(), 1);
    assert_eq!(saved.brushes.len(), level.brushes.len());
    assert_eq!(saved.volumes[0].bounds(), level.volumes[0].bounds());
    assert!(format::read(&format::write(&saved)).is_ok());
}

#[test]
fn a_volume_keeps_its_place_among_the_brushes() {
    let mut st = room_state();
    st.add_box([-2.0, -2.0, 0.0], [2.0, 2.0, 1.0]);
    let volume = Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    let id = st.doc.alloc_id();
    let def = super::document::BrushDef {
        id,
        shape: Shape::Volume(volume),
        op: Op::Add,
        material: 0,
        texinfos: Vec::new(),
    };
    st.doc.insert_brush(1, def);
    let level = st.doc.to_level().expect("exports");
    assert_eq!(level.brushes[0].order(), 0);
    assert_eq!(level.volumes[0].order(), 1);
    assert_eq!(level.brushes[1].order(), 2);
    let back = Document::from_level(&level);
    assert!(back.brushes()[1].is_volume());
}

#[test]
fn slicing_and_carving_refuse_a_volume() {
    let mut st = room_state();
    stroke(&mut st, &[at_wall(0.0)]);
    let def = st.doc.volumes().next().expect("volume").0.clone();
    assert!(super::shape_ops::parts(&def).is_none());
    st.selection = Some(super::state::Selection::Brush(def.id));
    assert!(!st.carve_selected());
    assert!(st.doc.brush(def.id).is_some());
}

#[test]
fn carving_a_brush_wall_makes_a_subtract_volume_that_bites() {
    let mut st = room_state();
    assert!(st.doc.volumes().next().is_none());
    stroke(&mut st, &[at_wall(0.0)]);
    let volume = the_volume(&st);
    assert_eq!(volume.op(), Op::Subtract);
    assert!(volume.holds(v(0.0, 8.0, 3.0)));
    // A ray into the bite goes past where the wall was.
    let level = st.doc.to_level().expect("exports");
    let brushes = level.static_brushes();
    let mut ctx = Context::new(&volume, &brushes);
    let t = mscent_world::voxel::first_solid(
        &volume,
        &mut ctx,
        v(0.0, 0.0, 3.0),
        v(0.0, 1.0, 0.0),
        100.0,
    )
    .expect("hits");
    assert!(t > 8.5, "the wall was at 8, now at {t}");
    assert!(surface(&st) > 0);
}

#[test]
fn adding_makes_an_add_volume_and_one_stroke_is_one_undo_step() {
    let mut st = room_state();
    st.voxel.op = VoxelOp::Add;
    let before = st.doc.clone();
    let depth = st.history.depth();
    stroke(&mut st, &[at_wall(0.0), at_wall(1.0), at_wall(2.0)]);
    assert_eq!(the_volume(&st).op(), Op::Add);
    assert_eq!(st.history.depth(), depth + 1);
    let after = st.doc.clone();
    assert!(st.undo());
    assert!(st.doc.volumes().next().is_none());
    assert_eq!(st.doc.brushes(), before.brushes());
    assert!(st.redo());
    assert_eq!(st.doc.brushes(), after.brushes());
}

#[test]
fn scrubbing_an_area_keeps_one_even_depth() {
    let mut once = room_state();
    stroke(&mut once, &[at_wall(0.0)]);
    let mut many = room_state();
    let rays: Vec<Ray> = (0..40).map(|_| at_wall(0.0)).collect();
    stroke(&mut many, &rays);
    assert_eq!(
        surface(&once),
        surface(&many),
        "picks read the start of the stroke"
    );
    assert_eq!(the_volume(&once), the_volume(&many));
}

#[test]
fn holding_alt_builds_up_instead() {
    let mut st = room_state();
    assert!(st.begin_stroke(
        at_wall(0.0),
        StrokeKeys {
            swap: false,
            build_up: true,
            ..Default::default()
        }
    ));
    let mut even = room_state();
    stroke(&mut even, &[at_wall(0.0)]);
    for _ in 0..12 {
        st.stroke_ray(at_wall(0.0));
    }
    st.finish_stroke();
    assert_ne!(the_volume(&st), the_volume(&even));
}

#[test]
fn control_swaps_add_and_carve() {
    let mut st = room_state();
    st.voxel.op = VoxelOp::Add;
    assert!(st.begin_stroke(
        at_wall(0.0),
        StrokeKeys {
            swap: true,
            build_up: false,
            ..Default::default()
        }
    ));
    st.finish_stroke();
    assert_eq!(the_volume(&st).op(), Op::Subtract);
}

#[test]
fn escape_puts_back_what_the_stroke_started_from() {
    let mut st = room_state();
    stroke(&mut st, &[at_wall(0.0)]);
    let kept = st.doc.clone();
    let depth = st.history.depth();
    st.begin_stroke(at_wall(3.0), StrokeKeys::default());
    st.stroke_ray(at_wall(4.0));
    std::thread::sleep(std::time::Duration::from_millis(30));
    st.poll_stroke();
    assert!(st.cancel_stroke());
    assert_eq!(st.doc.brushes(), kept.brushes());
    assert_eq!(st.history.depth(), depth);
    // A first stroke leaves no volume behind.
    let mut fresh = room_state();
    fresh.begin_stroke(at_wall(0.0), StrokeKeys::default());
    std::thread::sleep(std::time::Duration::from_millis(30));
    fresh.poll_stroke();
    assert!(fresh.cancel_stroke());
    assert!(fresh.doc.volumes().next().is_none());
}

#[test]
fn dabbing_past_the_box_grows_it_by_whole_chunks() {
    let mut st = room_state();
    stroke(&mut st, &[at_wall(0.0)]);
    let small = the_volume(&st).chunks();
    let rays: Vec<Ray> = (0..8).map(|i| at_wall(f64::from(i))).collect();
    stroke(&mut st, &rays);
    let big = the_volume(&st).chunks();
    assert!(big[0] > small[0], "{small:?} then {big:?}");
    assert_eq!(st.doc.volumes().count(), 1, "the same volume grew");
}

#[test]
fn a_stroke_stops_at_a_neighbour_rather_than_overlap_it() {
    let mut st = room_state();
    stroke(&mut st, &[at_wall(-6.0)]);
    let rays: Vec<Ray> = (0..8).map(|i| at_wall(f64::from(i) + 0.5)).collect();
    stroke(&mut st, &rays);
    let level = st.doc.to_level().expect("exports");
    let bytes = format::write(&level);
    assert!(format::read(&bytes).is_ok(), "overlapping would not load");
}

#[test]
fn smooth_with_nothing_to_work_on_makes_nothing() {
    let mut st = room_state();
    st.voxel.op = VoxelOp::Smooth;
    let depth = st.history.depth();
    stroke(&mut st, &[at_wall(0.0)]);
    assert!(st.doc.volumes().next().is_none());
    assert_eq!(st.history.depth(), depth);
    assert!(st.message.contains("no volume"), "{}", st.message);
}

#[test]
fn painting_changes_no_shape() {
    let mut st = room_state();
    stroke(&mut st, &[at_wall(0.0)]);
    let before = st.doc.clone();
    st.voxel.op = VoxelOp::Paint;
    st.voxel.material = 2;
    stroke(&mut st, &[at_wall(0.0)]);
    assert_ne!(st.doc, before);
    assert!(st.doc.same_geometry(&before), "no re-bake for paint");
}

#[test]
fn turning_a_volume_over_keeps_what_was_sculpted() {
    let mut st = room_state();
    st.voxel.op = VoxelOp::Add;
    stroke(&mut st, &[at_wall(0.0)]);
    let id = st.doc.volumes().next().expect("volume").0.id;
    st.selection = Some(super::state::Selection::Brush(id));
    assert!(st.flip_selected());
    assert_eq!(the_volume(&st).op(), Op::Subtract);
    assert_eq!(st.doc.brush(id).expect("still there").op, Op::Subtract);
    assert!(st.undo());
    assert_eq!(the_volume(&st).op(), Op::Add);
}

#[test]
fn moving_a_volume_goes_by_whole_cells_and_not_into_another() {
    let mut level = Level::default();
    let a = Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    let b = Volume::new(Op::Add, 1, CellSize::Quarter, [32, 0, 0], [1, 1, 1]).expect("valid");
    level.volumes = vec![a, b];
    let mut st = EditorState::new(Document::from_level(&level));
    let id = st.doc.brushes()[0].id;
    st.selection = Some(super::state::Selection::Brush(id));
    let mut moved = st.doc.brush(id).expect("first").clone();
    moved.translate([1.1, 0.0, 0.0]);
    assert_eq!(moved.bounds().0[0], 1.0);
    st.doc.replace_brush(&moved);
    let (first, second): (Vec<_>, Vec<_>) = st.doc.volumes().partition(|(d, _)| d.id == id);
    assert!(!first[0].1.overlaps(second[0].1));
}

#[test]
fn a_face_behind_a_voxel_surface_cannot_be_picked() {
    let mut st = room_state();
    st.voxel.op = VoxelOp::Add;
    let ahead = at_wall(0.0);
    assert!(super::pick::pick_visible_face(&st.doc, &ahead).is_some());
    stroke(&mut st, &[ahead]);
    let volume = the_volume(&st);
    let mut grown = volume.clone();
    let shape = volume.clone();
    let wall = Prim::cuboid(v(-1.0, 6.0, 2.0), v(1.0, 7.0, 4.0)).expect("valid");
    let brushes = st
        .doc
        .to_level()
        .expect("exports")
        .static_brushes()
        .to_vec();
    apply_dab(
        &mut grown,
        &mut Context::new(&shape, &brushes),
        &wall,
        DabOp::Add { material: 1 },
    );
    let mut def = st.doc.volumes().next().expect("volume").0.clone();
    def.shape = Shape::Volume(grown);
    st.doc.replace_brush(&def);
    assert!(super::pick::pick_visible_face(&st.doc, &ahead).is_none());
    assert!(super::pick::pick_face(&st.doc, &ahead).is_some());
}

#[test]
fn a_level_past_the_file_limits_cannot_be_saved_as_if_it_were_fine() {
    // Two overlapping volumes, which no tool makes, stand in for any level the
    // loader would refuse: the old file must stay.
    let dir = std::env::temp_dir().join(format!("mscent-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("level.msl");
    let mut level = Level::default();
    let a = Volume::new(Op::Add, 0, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    let b = Volume::new(Op::Add, 1, CellSize::Quarter, [8, 0, 0], [1, 1, 1]).expect("valid");
    level.volumes = vec![a, b];
    let doc = Document::from_level(&level);
    let why = crate::files::save(&doc, &path).expect_err("refused");
    assert!(why.contains("cannot save"), "{why}");
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_press_does_not_throw_away_a_stroke_that_is_finishing() {
    let mut st = room_state();
    assert!(st.begin_stroke(at_wall(0.0), StrokeKeys::default()));
    st.end_stroke();
    // Whether the first is done by now or not, it must survive the second press.
    let _ = st.begin_stroke(at_wall(1.0), StrokeKeys::default());
    st.finish_stroke();
    assert_eq!(st.doc.volumes().count(), 1);
    assert!(st.history.depth() >= 2, "the room and the first stroke");
}

#[test]
fn an_invalid_brush_does_not_stop_face_picking() {
    let mut st = room_state();
    let ray = at_wall(0.0);
    let mut broken = st.doc.brushes()[0].clone();
    broken.id = st.doc.alloc_id();
    broken.shape = Shape::Box {
        min: [1.0, 1.0, 1.0],
        max: [0.0, 0.0, 0.0],
    };
    st.doc.insert_brush(1, broken);
    assert!(super::pick::pick_visible_face(&st.doc, &ray).is_some());
}
