//! What a curved surface must not show of the triangles it is made of, or of the
//! projections that texture it. Both are measured exactly: a highlight against the
//! same ball drawn by the other path, and spots against the volume they come from.

use mscent_math::Vec3;
use mscent_world::materials::CAP;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab};
use mscent_world::{Brush, Level, Op};

use super::fill::{FillContext, fill_voxels};
use super::list::VoxelList;
use super::solid::spots_of;
use super::{IN_SEAM, VertexLight, VoxelScene};
use crate::{Camera, Framebuffer};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const SKY: u32 = 0xff00_0000;
const SKIN: u32 = 0xff80_1010;
/// Wax, as the renderer's table has it.
const GLOSSY: u8 = 90;

fn v(x: f64, y: f64, z: f64) -> mscent_math::Vec3d {
    mscent_math::Vec3d::new(x, y, z)
}

/// A ball of `material` under one steady light.
fn ball(material: u8) -> VoxelScene {
    let brushes =
        vec![Brush::cuboid(v(-8.0, -8.0, -8.0), v(8.0, 8.0, 8.0), Op::Subtract, 0).expect("valid")];
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-16, -16, -16], [2, 2, 2]).expect("valid");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    let prim = Prim::sphere(v(0.0, 0.0, 0.0), 3.0).expect("valid");
    apply_dab(&mut volume, &mut ctx, &prim, DabOp::Add { material });
    let level = Level {
        brushes,
        volumes: vec![volume],
        ..Default::default()
    };
    let bsp = mscent_world::compile(&level.csg_brushes()).expect("compiles");
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    VoxelScene::new(world, bsp.leaves().len(), |_, m| {
        let lit = VertexLight {
            color: Vec3::new(200.0, 200.0, 200.0),
            dir: Vec3::new(0.4, -0.7, 0.6),
        };
        vec![lit; m.vertices.len()]
    })
}

const EYE: Vec3 = Vec3::new(1.0, -7.0, 1.5);

fn camera() -> Camera {
    Camera::look_at(
        EYE,
        Vec3::ZERO,
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        WIDTH,
        HEIGHT,
    )
    .expect("camera")
}

fn frame(scene: &VoxelScene, texture: &crate::Texture, spec: &[crate::SpecTable]) -> Framebuffer {
    let cam = camera();
    let mut list = VoxelList::default();
    list.candidates = (0..scene.world.chunks.len() as u32).collect();
    list.prepare(scene, &cam, EYE, |_| 0);
    let texture_of = |_: u16| texture;
    let ctx = FillContext {
        scene,
        cam: &cam,
        eye: EYE,
        fog: None,
        bump: false,
        specular: true,
        lights: &[],
        spec,
        texture_of: &texture_of,
    };
    let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("size");
    fb.clear_sky(SKY, SKY);
    fill_voxels(&mut fb, &list, &ctx);
    fb
}

fn flat(color: u32) -> crate::Texture {
    crate::Texture::new(128, 128, vec![color; 128 * 128]).expect("texture")
}

/// The same ball with every vertex sent down the blended path.
fn all_blended(mut scene: VoxelScene) -> VoxelScene {
    for lods in &mut scene.lods {
        for lod in lods {
            lod.axis.iter_mut().for_each(|a| *a = IN_SEAM);
        }
    }
    scene
}

/// A plain triangle and a blended one meet all over a curved surface, so any
/// difference in their highlight is drawn along the triangles between them: the
/// patches around the hive's cells. A flat texture leaves nothing else to differ.
#[test]
fn both_paths_draw_the_same_highlight() {
    let spec = [crate::SpecTable::new(GLOSSY)];
    let texture = flat(0xff80_8080);
    let plain = frame(&ball(0), &texture, &spec);
    let blended = frame(&all_blended(ball(0)), &texture, &spec);
    let mut lit = 0;
    let mut off = 0;
    for (&a, &b) in plain.pixels().iter().zip(blended.pixels()) {
        if a == SKY || b == SKY {
            continue;
        }
        lit += 1;
        let gap = (0..3)
            .map(|c| (i32::from((a >> (8 * c)) as u8) - i32::from((b >> (8 * c)) as u8)).abs())
            .max()
            .unwrap_or(0);
        if gap > 4 {
            off += 1;
        }
    }
    assert!(lit > 10_000, "only {lit} pixels of ball");
    assert!(
        off * 200 < lit,
        "{off} of {lit} pixels differ between paths"
    );
}

/// Whether a pixel shows spot rather than skin: spots are the only green here.
fn is_spot(p: u32) -> bool {
    ((p >> 8) & 255) * 2 > (p >> 16) & 255
}

/// Every vertex facing the eye is checked against the volume at its own point, so
/// a spot that a projection stretched or cut is caught wherever it happened.
#[test]
fn spots_are_where_the_volume_says_across_the_whole_cap() {
    let material = CAP as u8;
    let scene = ball(material);
    let fb = frame(&scene, &flat(SKIN), &[crate::SpecTable::new(0)]);
    let spots = spots_of(material).expect("a cap has spots");
    let cam = camera();
    let (mut inside, mut outside, mut wrong) = (0, 0, 0);
    for chunk in &scene.world.chunks {
        for vertex in &chunk.lods[0].vertices {
            if vertex.normal.dot(EYE - vertex.pos) <= 0.0 {
                continue;
            }
            let Some(s) = cam.project(cam.world_to_camera(vertex.pos)) else {
                continue;
            };
            let (x, y) = (s.x as u32, s.y as u32);
            if x >= WIDTH || y >= HEIGHT {
                continue;
            }
            let shown = is_spot(fb.pixels()[(y * WIDTH + x) as usize]);
            // Only well inside or outside: a triangle's own flatness moves an edge.
            match spots.coverage(vertex.pos, 0.02) {
                c if c > 0.95 => {
                    inside += 1;
                    wrong += i32::from(!shown);
                }
                c if c < 0.05 => {
                    outside += 1;
                    wrong += i32::from(shown);
                }
                _ => {}
            }
        }
    }
    assert!(inside >= 30, "only {inside} vertices in a spot");
    assert!(outside >= 300, "only {outside} vertices clear of one");
    assert!(
        wrong * 50 < inside + outside,
        "{wrong} of {} disagree with the volume",
        inside + outside
    );
}

/// Any other material draws no spots at all.
#[test]
fn only_a_cap_draws_spots() {
    let fb = frame(&ball(0), &flat(SKIN), &[crate::SpecTable::new(0)]);
    let spots = fb.pixels().iter().filter(|&&p| p != SKY && is_spot(p));
    assert_eq!(spots.count(), 0);
}
