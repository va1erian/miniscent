//! How voxel triangles are shaded, whichever path they take.

use std::path::Path;

use mscent_math::Vec3;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab, fill};
use mscent_world::{Brush, Level, Op};

use crate::Framebuffer;

fn v(x: f64, y: f64, z: f64) -> mscent_math::Vec3d {
    mscent_math::Vec3d::new(x, y, z)
}

fn room() -> Brush {
    Brush::cuboid(v(-8.0, -8.0, 0.0), v(8.0, 8.0, 6.0), Op::Subtract, 0).expect("valid")
}

/// A closed gyroid cave in solid rock.
fn gyroid_level() -> Level {
    let g = |p: mscent_math::Vec3d| (p.x * 0.55).sin() * (p.y * 0.55).cos() + 0.25;
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [4, 4, 2]).expect("valid");
    let shape = volume.clone();
    fill(&mut volume, &mut Context::new(&shape, &[]), g, 4);
    Level {
        volumes: vec![volume],
        ..Default::default()
    }
}

/// One frame of `scene` alone, on the plain or the blended path as its data says.
fn voxel_frame(
    scene: &crate::voxel::VoxelScene,
    texture: &crate::Texture,
    spec: &[crate::SpecTable],
) -> Framebuffer {
    use crate::voxel::fill::{FillContext, fill_voxels};
    use crate::voxel::list::VoxelList;
    let eye = Vec3::new(0.0, -3.0, 4.0);
    let cam = crate::Camera::look_at(
        eye,
        Vec3::new(0.0, 0.0, 0.5),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        320,
        180,
    )
    .expect("camera");
    let mut list = VoxelList::default();
    list.candidates = (0..scene.world.chunks.len() as u32).collect();
    list.prepare(scene, &cam, eye, |_| 0);
    let texture_of = |_: u16| texture;
    let ctx = FillContext {
        scene,
        cam: &cam,
        eye,
        fog: None,
        bump: true,
        specular: true,
        lights: &[],
        spec,
        texture_of: &texture_of,
    };
    let mut fb = Framebuffer::new(320, 180).expect("fb");
    fill_voxels(&mut fb, &list, &ctx);
    fb
}

/// A triangle whose corners disagree on material must shade as one whose corners
/// agree does, or it shows as a patch on anything glossy. Both materials here are
/// the same texture, so any difference is the path it went by.
#[test]
fn triangles_with_mixed_corners_shade_like_their_neighbours() {
    use crate::voxel::{VertexLight, VoxelScene};
    use mscent_world::voxel::VoxelWorld;
    let brushes = vec![room()];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let top = Prim::cuboid(v(-3.9, -3.9, -1.0), v(3.9, 3.9, 0.6)).expect("valid");
    apply_dab(&mut volume, &mut ctx, &top, DabOp::Add { material: 1 });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let light = |_: usize, m: &mscent_world::voxel::ChunkMesh| {
        let lit = VertexLight {
            color: Vec3::new(150.0, 150.0, 150.0),
            dir: Vec3::new(0.2, -0.5, 0.84),
        };
        vec![lit; m.vertices.len()]
    };
    let plain = VoxelScene::new(world.clone(), bsp.leaves().len(), light);
    let mut mixed = plain.clone();
    for lods in &mut mixed.lods {
        for data in lods.iter_mut() {
            for (i, m) in data.material.iter_mut().enumerate() {
                *m = (i % 2) as u8;
            }
        }
    }
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let texture = crate::load_texture(&dir, "wax").expect("wax").filtered();
    let spec = [crate::SpecTable::new(200), crate::SpecTable::new(200)];
    let (a, b) = (
        voxel_frame(&plain, &texture, &spec),
        voxel_frame(&mixed, &texture, &spec),
    );
    let lum = |p: u32| ((p >> 16) & 255) as i32 + ((p >> 8) & 255) as i32 + (p & 255) as i32;
    let (mut sum, mut count) = (0i64, 0i64);
    for (x, y) in a.pixels().iter().zip(b.pixels()) {
        if *x != 0xff00_0000 || *y != 0xff00_0000 {
            sum += i64::from((lum(*x) - lum(*y)).abs());
            count += 1;
        }
    }
    assert!(count > 5000, "the surface is on screen: {count} pixels");
    let mean = sum as f64 / count as f64;
    assert!(mean < 6.0, "the paths differ by {mean} a pixel");
}

/// A cell's record can be dark among lit ones. Standing for all of them, a merged
/// vertex must not show that as a black block at a distance.
#[test]
fn a_merged_vertex_takes_the_mean_of_the_cells_it_stands_for() {
    use crate::voxel::{VertexLight, VoxelScene};
    use mscent_world::voxel::VoxelWorld;
    let level = gyroid_level();
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    // A checkerboard of lit and dark cells.
    let scene = VoxelScene::new(world, bsp.leaves().len(), |_, m| {
        m.vertices
            .iter()
            .map(|v| {
                let odd = (v.cell[0] + v.cell[1] + v.cell[2]).rem_euclid(2) == 1;
                let c = if odd { 200.0 } else { 0.0 };
                VertexLight {
                    color: Vec3::new(c, c, c),
                    dir: v.normal,
                }
            })
            .collect()
    });
    let (mut between, mut total) = (0, 0);
    for (chunk, data) in scene.world.chunks.iter().zip(&scene.lods) {
        let interior = |c: [i8; 3]| c.iter().all(|&v| (2..12).contains(&v));
        for (v, l) in chunk.lods[2].vertices.iter().zip(&data[2].light) {
            if interior(v.cell) {
                total += 1;
                between += usize::from(l.color.x > 40.0 && l.color.x < 160.0);
            }
        }
    }
    assert!(total > 40, "{total} interior vertices");
    assert!(
        between * 10 > total * 8,
        "{between} of {total} merged vertices are in between"
    );
}
