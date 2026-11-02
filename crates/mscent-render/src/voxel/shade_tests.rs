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
    let at = Vec3::new(0.0, -3.0, 4.0);
    voxel_frame_from(scene, texture, spec, at)
}

fn voxel_frame_from(
    scene: &crate::voxel::VoxelScene,
    texture: &crate::Texture,
    spec: &[crate::SpecTable],
    eye: Vec3,
) -> Framebuffer {
    use crate::voxel::fill::{FillContext, fill_voxels};
    use crate::voxel::list::VoxelList;
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

/// A ball in a room with a light that does not vary, and a bold checker for a
/// texture: all that is left to see is how the projections meet.
fn checker_ball() -> Framebuffer {
    use crate::voxel::{VertexLight, VoxelScene};
    use mscent_world::voxel::VoxelWorld;
    let brushes = vec![room()];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let ball = Prim::sphere(v(0.0, 0.0, 1.0), 2.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 0 });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let scene = VoxelScene::new(world, bsp.leaves().len(), |_, m| {
        let lit = VertexLight {
            color: Vec3::new(256.0, 256.0, 256.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
        };
        vec![lit; m.vertices.len()]
    });
    let pixels: Vec<u32> = (0..128u32 * 128)
        .map(|i| {
            let (x, y) = (i % 128, i / 128);
            if (x / 8 + y / 8) % 2 == 0 {
                0xff20_2020
            } else {
                0xffe0_e0e0
            }
        })
        .collect();
    let texture = crate::Texture::new(128, 128, pixels).expect("texture");
    voxel_frame(&scene, &texture, &[crate::SpecTable::new(0)])
}

/// How many pixels stand alone against all four of their neighbours: the speckle
/// a dither leaves in a pattern.
fn speckles(fb: &Framebuffer) -> usize {
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let lum = |p: u32| i32::from((p & 255) as u8);
    let px = fb.pixels();
    let mut count = 0;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let at = lum(px[y * w + x]);
            let near = [
                px[y * w + x - 1],
                px[y * w + x + 1],
                px[(y - 1) * w + x],
                px[(y + 1) * w + x],
            ];
            if near.iter().all(|&n| (lum(n) - at).abs() > 80) {
                count += 1;
            }
        }
    }
    count
}

/// Where projections meet on a curved surface they meet along a line, so a pattern
/// on the surface stays a pattern. A dither across whole triangles turned it to
/// noise, in squares that followed the voxel grid.
#[test]
fn a_pattern_on_a_curved_surface_stays_clean_where_projections_meet() {
    let fb = checker_ball();
    let n = speckles(&fb);
    // Dithering across the triangles gave 304 here. A few are left where a seam
    // crosses the checker's own corners.
    assert!(n < 60, "{n} speckled pixels");
}

/// A chunk at a lower level of detail must keep its bump mapping, or the border
/// with a nearer chunk shows as a hard line between textured and flat rock.
#[test]
fn a_far_chunk_keeps_its_bump_mapping() {
    use crate::voxel::{VertexLight, VoxelScene};
    use mscent_world::voxel::VoxelWorld;
    let brushes = vec![
        Brush::cuboid(v(-40.0, -40.0, 0.0), v(40.0, 40.0, 40.0), Op::Subtract, 0).expect("valid"),
    ];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let top = Prim::cuboid(v(-3.9, -3.9, -1.0), v(3.9, 3.9, 0.6)).expect("valid");
    apply_dab(&mut volume, &mut ctx, &top, DabOp::Add { material: 0 });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let scene = VoxelScene::new(world, bsp.leaves().len(), |_, m| {
        let lit = VertexLight {
            color: Vec3::new(150.0, 150.0, 150.0),
            dir: Vec3::new(0.5, -0.3, 0.8),
        };
        vec![lit; m.vertices.len()]
    });
    // Past where full detail ends, near enough that the bump has not faded.
    let eye = Vec3::new(0.0, -13.0, 9.0);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let bumpy = crate::load_texture(&dir, "rock").expect("rock").filtered();
    let pixels: Vec<u32> = (0..128 * 128).map(|_| 0xff80_8080).collect();
    let spec = [crate::SpecTable::new(0)];
    let flat_map = crate::Texture::new(128, 128, pixels.clone()).expect("flat");
    let mut with_map = flat_map.clone();
    let normals: Vec<u32> = (0..bumpy.height() as i32)
        .flat_map(|y| {
            let b = &bumpy;
            (0..b.width() as i32).map(move |x| b.normal_texel(x, y).unwrap_or(0xff80_80ff))
        })
        .collect();
    with_map = with_map.with_normals(normals).expect("normals");
    let (a, b) = (
        voxel_frame_from(&scene, &with_map, &spec, eye),
        voxel_frame_from(&scene, &flat_map, &spec, eye),
    );
    let differing = a
        .pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(x, y)| x != y)
        .count();
    assert!(differing > 500, "only {differing} pixels show the bump");
}

/// How many neighbouring pixels jump by more than a texture's own grain does, on a
/// ball of rock in flat light. A hard cut between two projections shows here.
#[test]
fn projections_fade_into_each_other_with_no_hard_cut() {
    use crate::voxel::{VertexLight, VoxelScene};
    use mscent_world::voxel::VoxelWorld;
    let brushes = vec![room()];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let ball = Prim::sphere(v(0.0, 0.0, 1.0), 2.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material: 0 });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    let scene = VoxelScene::new(world, bsp.leaves().len(), |_, m| {
        let lit = VertexLight {
            color: Vec3::new(256.0, 256.0, 256.0),
            dir: Vec3::new(0.0, 0.0, 1.0),
        };
        vec![lit; m.vertices.len()]
    });
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let rock = crate::load_texture(&dir, "rock").expect("rock");
    let mut pixels = Vec::new();
    for y in 0..rock.height() as i32 {
        for x in 0..rock.width() as i32 {
            pixels.push(rock.texel(x, y));
        }
    }
    let plain = crate::Texture::new(rock.width(), rock.height(), pixels)
        .expect("texture")
        .filtered();
    let fb = voxel_frame(&scene, &plain, &[crate::SpecTable::new(0)]);
    let (w, h) = (fb.width() as usize, fb.height() as usize);
    let px = fb.pixels();
    let lum = |p: u32| i32::from((p & 255) as u8);
    let count = |limit: i32| {
        let mut jumps = 0;
        for y in 0..h - 1 {
            for x in 0..w - 1 {
                let at = px[y * w + x];
                if at == 0xff00_0000 {
                    continue;
                }
                for n in [px[y * w + x + 1], px[(y + 1) * w + x]] {
                    jumps += usize::from(n != 0xff00_0000 && (lum(at) - lum(n)).abs() > limit);
                }
            }
        }
        jumps
    };
    // A hard cut gave 39 here. Rock's own grain stays well under this step.
    let jumps = count(10);
    assert!(jumps < 15, "{jumps} hard jumps");
}
