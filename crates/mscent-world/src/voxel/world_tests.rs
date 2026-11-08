//! Volumes as a level compiles them: shells, brush materials, buried triangles
//! and tunnels crossing from one volume into the next.

use mscent_math::{Vec3, Vec3d};

use super::test_util::{coords, open_edges};
use super::{
    CellSize, ChunkMesh, Context, DabOp, FALLBACK_MATERIAL, Mesher, Prim, Volume, VoxelWorld,
    apply_dab,
};
use crate::brush::{Brush, Op};
use crate::bsp::LeafKind;
use crate::compile;
use crate::entity::Level;
use crate::materials::{BRICK, METAL, NO_DRAW, ROCK};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn room(material: u16) -> Brush {
    Brush::cuboid(v(-8.0, -8.0, 0.0), v(8.0, 0.0, 6.0), Op::Subtract, 0)
        .expect("valid")
        .with_material(material)
}

fn world_of(level: &Level) -> VoxelWorld {
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    VoxelWorld::build(level, &level.static_brushes(), &bsp)
}

fn level_with(brushes: Vec<Brush>, volumes: Vec<Volume>) -> Level {
    Level {
        brushes,
        volumes,
        ..Default::default()
    }
}

fn full_meshes(world: &VoxelWorld) -> impl Iterator<Item = &ChunkMesh> {
    world.chunks.iter().map(|c| &c.lods[0])
}

/// A volume straddling the room's +Y wall, holding air on one side and rock on
/// the other.
fn wall_volume() -> Volume {
    Volume::new(Op::Subtract, 1, CellSize::Quarter, [-16, -16, 0], [2, 2, 1]).expect("valid")
}

/// Whether the segment passes through a triangle of `mesh`.
fn hits(mesh: &ChunkMesh, from: Vec3, to: Vec3) -> bool {
    let dir = to - from;
    mesh.triangles.iter().any(|t| {
        let [a, b, c] = t.map(|i| mesh.vertices[usize::from(i)].pos);
        let (e1, e2) = (b - a, c - a);
        let h = dir.cross(e2);
        let det = e1.dot(h);
        if det.abs() < 1.0e-7 {
            return false;
        }
        let s = (from - a) * (1.0 / det);
        let u = s.dot(h);
        let q = s.cross(e1);
        let w = dir.dot(q);
        let t = e2.dot(q);
        (0.0..=1.0).contains(&u) && w >= 0.0 && u + w <= 1.0 && (0.0..=1.0).contains(&t)
    })
}

#[test]
fn a_subtract_volume_adds_a_shell_that_is_never_drawn_and_an_add_volume_adds_nothing() {
    let sub = wall_volume();
    let add = Volume::new(Op::Add, 2, CellSize::Quarter, [64, 0, 0], [1, 1, 1]).expect("valid");
    let level = level_with(vec![room(ROCK)], vec![sub, add]);
    let all = level.csg_brushes();
    assert_eq!(all.len(), 2, "a room and one shell");
    assert_eq!(all[1].op(), Op::Subtract);
    assert_eq!(all[1].order(), 1);
    assert_eq!(all[1].material(), NO_DRAW);
    let plain = level_with(vec![room(ROCK)], Vec::new());
    assert_eq!(plain.csg_brushes().len(), 1);
}

#[test]
fn an_untouched_subtract_volume_shows_the_material_of_the_wall_it_covers() {
    let level = level_with(vec![room(METAL)], vec![wall_volume()]);
    let world = world_of(&level);
    let materials: Vec<u8> = full_meshes(&world)
        .flat_map(|m| m.materials.iter().copied())
        .collect();
    assert!(!materials.is_empty());
    assert!(
        materials.iter().all(|&m| u16::from(m) == METAL),
        "{materials:?}"
    );
}

#[test]
fn a_carved_wall_shows_the_material_of_the_brush_it_came_from() {
    let brushes = vec![room(BRICK)];
    let mut volume = wall_volume();
    let mut ctx = Context::new(&volume, &brushes);
    let tunnel = Prim::sphere(v(0.0, 1.0, 2.0), 1.2).expect("valid");
    apply_dab(&mut volume, &mut ctx, &tunnel, DabOp::Carve);
    let world = world_of(&level_with(brushes.clone(), vec![volume]));
    let deep: Vec<u8> = full_meshes(&world)
        .flat_map(|m| {
            m.triangles
                .iter()
                .zip(&m.materials)
                .filter_map(|(t, &mat)| {
                    let p = m.vertices[usize::from(t[0])].pos;
                    (p.y > 0.5).then_some(mat)
                })
        })
        .collect();
    assert!(!deep.is_empty(), "the carve left a surface in the rock");
    assert!(deep.iter().all(|&m| u16::from(m) == BRICK), "{deep:?}");
}

#[test]
fn rock_too_deep_for_any_brush_still_looks_like_the_nearest_one() {
    let brushes = vec![room(METAL)];
    let mut volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [-16, 0, 0], [2, 4, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    let far = Prim::sphere(v(0.0, 10.0, 2.0), 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &far, DabOp::Carve);
    let world = world_of(&level_with(brushes, vec![volume]));
    let materials: Vec<u8> = full_meshes(&world)
        .flat_map(|m| m.materials.iter().copied())
        .collect();
    assert!(!materials.is_empty());
    assert!(materials.iter().all(|&m| u16::from(m) == METAL));
}

#[test]
fn paint_beats_the_material_a_sample_would_inherit() {
    let brushes = vec![room(METAL)];
    let mut volume = wall_volume();
    let mut ctx = Context::new(&volume, &brushes);
    let spot = Prim::sphere(v(0.0, 0.0, 2.0), 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &spot, DabOp::Paint { material: 2 });
    let world = world_of(&level_with(brushes, vec![volume]));
    let painted = full_meshes(&world)
        .flat_map(|m| m.materials.iter())
        .filter(|&&m| m == 2)
        .count();
    assert!(painted > 0);
}

#[test]
fn a_brush_material_samples_cannot_name_draws_with_the_fallback() {
    let level = level_with(vec![room(300)], vec![wall_volume()]);
    let world = world_of(&level);
    let materials: Vec<u8> = full_meshes(&world)
        .flat_map(|m| m.materials.iter().copied())
        .collect();
    assert!(!materials.is_empty());
    assert!(materials.iter().all(|&m| m == FALLBACK_MATERIAL));
}

#[test]
fn triangles_inside_brush_solid_are_dropped_and_those_in_the_room_stay() {
    let brushes = vec![room(ROCK)];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-8, -24, -16], [2, 2, 2]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    // A ball half in the floor.
    let ball = Prim::sphere(v(-2.0, -4.0, 0.0), 1.5).expect("valid");
    apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    let mut mesher = Mesher::new();
    let before: usize = coords(&volume)
        .into_iter()
        .filter_map(|c| mesher.mesh(&volume, &mut ctx, c))
        .map(|m| m.triangles.len())
        .sum();
    let world = world_of(&level_with(brushes, vec![volume]));
    let after: usize = full_meshes(&world).map(|m| m.triangles.len()).sum();
    assert!(
        after > before / 4,
        "{after} of {before}: the top half stays"
    );
    assert!(after < before * 3 / 4, "{after} of {before}: the rest went");
    for m in full_meshes(&world) {
        let lowest = m.vertices.iter().map(|v| v.pos.z).fold(f32::MAX, f32::min);
        assert!(lowest > -0.5, "buried vertices stayed: {lowest}");
    }
}

#[test]
fn a_chunk_wholly_inside_brush_solid_has_no_geometry() {
    let brushes = vec![room(ROCK)];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-8, 64, 0], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&volume, &brushes);
    let ball = Prim::sphere(v(0.0, 18.0, 2.0), 1.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 1 });
    let world = world_of(&level_with(brushes, vec![volume]));
    assert!(world.chunks.is_empty());
    assert_eq!(world.volumes[0].table, [super::NO_CHUNK]);
}

#[test]
fn every_chunk_names_only_empty_leaves_and_at_least_one() {
    let level = level_with(vec![room(ROCK)], vec![wall_volume()]);
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    assert!(!world.chunks.is_empty());
    for chunk in &world.chunks {
        assert!(!chunk.leaves.is_empty());
        for &leaf in &chunk.leaves {
            assert_eq!(bsp.leaf_kind(leaf), LeafKind::Empty);
        }
        assert!(chunk.leaves.windows(2).all(|w| w[0].0 < w[1].0));
    }
}

/// Two volumes side by side in solid rock, meeting at x = 4, with a tunnel
/// carved through both along X.
fn tunnelled_pair(second: CellSize) -> Vec<Volume> {
    let side = second.units();
    let origin = (4.0 / side) as i32;
    let mut a =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0, -8, -8], [1, 1, 1]).expect("valid");
    let chunks = (4.0 / (side * 16.0)) as u16;
    let mut b = Volume::new(
        Op::Subtract,
        1,
        second,
        [origin, (-2.0 / side) as i32, (-2.0 / side) as i32],
        [chunks; 3],
    )
    .expect("valid");
    let tunnel = Prim::round_cone(v(1.0, 0.0, 0.0), v(7.0, 0.0, 0.0), 1.0, 1.0).expect("valid");
    let brushes: Vec<Brush> = Vec::new();
    let shape = a.clone();
    apply_dab(
        &mut a,
        &mut Context::new(&shape, &brushes),
        &tunnel,
        DabOp::Carve,
    );
    let shape = b.clone();
    apply_dab(
        &mut b,
        &mut Context::new(&shape, &brushes),
        &tunnel,
        DabOp::Carve,
    );
    vec![a, b]
}

#[test]
fn a_tunnel_across_two_touching_volumes_has_no_wall_in_it() {
    let world = world_of(&level_with(Vec::new(), tunnelled_pair(CellSize::Quarter)));
    let (from, to) = (Vec3::new(1.5, 0.0, 0.0), Vec3::new(6.5, 0.0, 0.0));
    assert!(!world.chunks.is_empty());
    for m in full_meshes(&world) {
        assert!(!hits(m, from, to), "a wall stands in the tunnel");
    }
    // The walls of the tunnel are still there.
    let (up_from, up_to) = (Vec3::new(4.0, 0.0, 0.0), Vec3::new(4.0, 0.0, 1.9));
    let side_a = Vec3::new(3.0, 0.0, 0.0);
    let side_b = Vec3::new(5.0, 0.0, 0.0);
    let any = |a: Vec3, b: Vec3| full_meshes(&world).any(|m| hits(m, a, b));
    assert!(any(side_a, side_a + (up_to - up_from)), "wall of the first");
    assert!(
        any(side_b, side_b + (up_to - up_from)),
        "wall of the second"
    );
}

#[test]
fn volumes_of_different_cell_sizes_do_not_wall_off_a_tunnel_either() {
    let world = world_of(&level_with(Vec::new(), tunnelled_pair(CellSize::Eighth)));
    let (from, to) = (Vec3::new(1.5, 0.0, 0.0), Vec3::new(6.5, 0.0, 0.0));
    for m in full_meshes(&world) {
        assert!(!hits(m, from, to), "a wall stands in the tunnel");
    }
}

#[test]
fn without_its_neighbour_the_same_tunnel_is_closed_off() {
    let pair = tunnelled_pair(CellSize::Quarter);
    let alone = level_with(Vec::new(), vec![pair[0].clone()]);
    let world = world_of(&alone);
    let (from, to) = (Vec3::new(1.5, 0.0, 0.0), Vec3::new(4.4, 0.0, 0.0));
    assert!(full_meshes(&world).any(|m| hits(m, from, to)));
}

#[test]
fn level_of_detail_keeps_only_what_the_full_mesh_kept() {
    let level = level_with(vec![room(ROCK)], vec![wall_volume()]);
    let world = world_of(&level);
    for chunk in &world.chunks {
        let [full, half, quarter] = &chunk.lods;
        assert!(half.triangles.len() <= full.triangles.len());
        assert!(quarter.triangles.len() <= half.triangles.len());
        assert_eq!(full.triangles.len(), full.materials.len());
    }
}

#[test]
fn the_surface_of_a_level_is_built_the_same_from_the_same_bytes() {
    let level = level_with(vec![room(ROCK)], tunnelled_pair(CellSize::Quarter));
    assert_eq!(world_of(&level), world_of(&level));
}

/// A tunnel carved along a volume sixteen chunks long: more chunks than one band.
fn long_tunnel() -> Volume {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0, -8, -8], [16, 1, 1]).expect("valid");
    let tunnel = Prim::round_cone(v(1.0, 0.0, 0.0), v(63.0, 0.0, 0.0), 1.5, 1.0).expect("valid");
    let shape = volume.clone();
    apply_dab(
        &mut volume,
        &mut Context::new(&shape, &[]),
        &tunnel,
        DabOp::Carve,
    );
    volume
}

#[test]
fn the_surface_is_the_same_on_one_thread_and_on_many() {
    let level = level_with(vec![room(ROCK)], vec![long_tunnel()]);
    let bsp = compile(&level.csg_brushes()).expect("compiles");
    let brushes = level.static_brushes();
    let one = VoxelWorld::build_on(1, &level, &brushes, &bsp);
    assert!(
        one.chunks.len() > 8,
        "{} chunks: several bands",
        one.chunks.len()
    );
    for workers in [2, 3, 8] {
        assert_eq!(VoxelWorld::build_on(workers, &level, &brushes, &bsp), one);
    }
}

#[test]
fn a_volume_alone_in_rock_stays_closed_through_the_world_path() {
    let pair = tunnelled_pair(CellSize::Quarter);
    let alone = pair[0].clone();
    let mut ctx = Context::new(&alone, &[]);
    let mut mesher = Mesher::new();
    let meshes: Vec<_> = coords(&alone)
        .into_iter()
        .map(|c| (c, mesher.mesh(&alone, &mut ctx, c).expect("chunk")))
        .collect();
    assert!(open_edges(&meshes).is_empty());
}
