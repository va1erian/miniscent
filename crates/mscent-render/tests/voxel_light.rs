//! Light on voxel surfaces, read from real pixels: voxels shadow brushes and
//! brushes shadow voxels, and the light does not step across the seam between a
//! brush floor and a voxel one.

#[allow(dead_code)]
#[path = "support/crawl.rs"]
mod crawl;
#[allow(dead_code)]
#[path = "support/stability.rs"]
mod stability;
#[allow(dead_code)]
#[path = "support/voxel_scene.rs"]
mod voxel_scene;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BuildConfig, Compiled, Framebuffer, LevelRenderer, Quality, View};
use mscent_world::brush::Op;
use mscent_world::materials::ROCK;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab};
use mscent_world::{Entity, Level};
use std::sync::Arc;

use stability::Look;
use voxel_scene::{cuboid, lamp};

const SIZE: (u32, u32) = (320, 180);

/// Flat grey textures and no fog, so the pixels are the light.
fn build(level: &Level, look: Look) -> LevelRenderer {
    let cfg = BuildConfig {
        luxel_size: 0.25,
        shadow_samples: 16,
        ..stability::config()
    };
    LevelRenderer::from_level(level, stability::materials_for(look), &cfg).expect("builds")
}

fn luma(c: u32) -> f32 {
    let (r, g, b) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Mean luminance of the `2 * reach + 1` pixel square around where the world
/// point `p` falls on the screen.
fn window(r: &LevelRenderer, view: &View, p: Vec3, reach: i32) -> f32 {
    let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("framebuffer");
    r.render(&mut fb, view, Quality::Low, &[], None)
        .expect("renders");
    let cam = r.camera(view, SIZE.0, SIZE.1).expect("camera");
    let at = cam.project(cam.world_to_camera(p)).expect("on screen");
    let (cx, cy) = (at.x as i32, at.y as i32);
    let mut sum = 0.0;
    for y in cy - reach..=cy + reach {
        for x in cx - reach..=cx + reach {
            let (x, y) = (x.clamp(0, SIZE.0 as i32 - 1), y.clamp(0, SIZE.1 as i32 - 1));
            sum += luma(fb.pixels()[y as usize * SIZE.0 as usize + x as usize]);
        }
    }
    sum / ((2 * reach + 1) * (2 * reach + 1)) as f32
}

/// A room 24 by 24 by 8 with a pillar of rock standing in it: a sphere of radius
/// 1.5 centred a unit above the floor, in a volume of its own.
fn pillar_room(at: [f64; 2], plate: bool) -> Level {
    let origin = [(at[0] - 2.0) as i32 * 4, (at[1] - 2.0) as i32 * 4, 0];
    let mut volume = Volume::new(Op::Add, 1, CellSize::Quarter, origin, [1, 1, 1]).expect("valid");
    let shape = volume.clone();
    let ball = Prim::sphere(Vec3d::new(at[0], at[1], 1.0), 1.5).expect("sphere");
    let rock = DabOp::Add {
        material: ROCK as u8,
    };
    apply_dab(&mut volume, &mut Context::new(&shape, &[]), &ball, rock);
    let mut level = Level {
        brushes: vec![cuboid([0.0; 3], [24.0, 24.0, 8.0], Op::Subtract, 0)],
        volumes: vec![volume],
        ..Default::default()
    };
    if plate {
        let lo = [at[0] - 3.0, at[1] - 3.0, 4.5];
        let hi = [at[0] + 3.0, at[1] + 3.0, 4.75];
        level.brushes.push(cuboid(lo, hi, Op::Add, 2));
    }
    level
}

fn lit(mut level: Level, lights: &[Entity]) -> Level {
    level.entities.extend_from_slice(lights);
    level
}

#[test]
fn a_voxel_pillar_shadows_the_brush_floor_behind_it() {
    // The lamp, the pillar and the floor point line up in that order.
    let lamp = [lamp(6.0, 12.0, 6.0)];
    let floor_point = Vec3::new(15.0, 12.0, 0.0);
    let eye = Vec3::new(15.0, 6.0, 5.0);
    let view = View::from_angles(eye, 0.0, -0.69, 0.0);
    let in_shadow = build(&lit(pillar_room([12.0, 12.0], false), &lamp), Look::Flat);
    let clear = build(&lit(pillar_room([12.0, 4.0], false), &lamp), Look::Flat);
    let (dark, bright) = (
        window(&in_shadow, &view, floor_point, 3),
        window(&clear, &view, floor_point, 3),
    );
    println!("floor behind the pillar: {dark:.1}, with the pillar elsewhere: {bright:.1}");
    assert!(bright > 40.0, "the floor is lit when nothing is in the way");
    assert!(dark < 0.2 * bright, "dark {dark}, bright {bright}");
}

#[test]
fn a_brush_plate_shadows_the_voxel_pillar_under_it() {
    // A point on the upper front of the pillar, which the lamp overhead lights.
    let lamp = [lamp(10.0, 10.0, 7.0)];
    let point = Vec3::new(10.0, 9.25, 2.3);
    let view = View::from_angles(Vec3::new(10.0, 4.0, 3.0), 0.0, -0.1, 0.0);
    let shaded = build(&lit(pillar_room([10.0, 10.0], true), &lamp), Look::Flat);
    let open = build(&lit(pillar_room([10.0, 10.0], false), &lamp), Look::Flat);
    let (dark, bright) = (
        window(&shaded, &view, point, 2),
        window(&open, &view, point, 2),
    );
    println!("pillar under a plate: {dark:.1}, open to the lamp: {bright:.1}");
    assert!(
        bright > 40.0,
        "the pillar is lit when nothing is in the way"
    );
    assert!(dark < 0.2 * bright, "dark {dark}, bright {bright}");
}

/// Mean luminance on either side of the seam between the brush floor (west of
/// `x = 8`) and the voxel floor, at the same distances from a lamp above it.
fn across_the_seam(look: Look) -> Vec<(f32, f32, f32)> {
    // Dim enough that the picture is not saturated, which would hide any step.
    let dim = Entity::Light {
        pos: Vec3::new(8.0, 8.0, 5.0),
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 60.0,
        intensity: 0.15,
    };
    let level = lit(voxel_scene::room(), &[dim]);
    let r = build(&level, look);
    let view = View::from_angles(Vec3::new(8.0, 2.5, 5.0), 0.0, -0.9, 0.0);
    [1.0f32, 2.0, 3.0]
        .into_iter()
        .map(|d| {
            let brush = window(&r, &view, Vec3::new(8.0 - d, 8.0, 0.0), 6);
            let voxel = window(&r, &view, Vec3::new(8.0 + d, 8.0, 0.0), 6);
            (d, brush, voxel)
        })
        .collect()
}

#[test]
fn the_light_does_not_step_across_the_seam_of_brush_and_voxel_floors() {
    for look in [Look::Flat, Look::Real] {
        for (d, brush, voxel) in across_the_seam(look) {
            let step = (voxel - brush).abs() / brush.max(voxel);
            println!(
                "{look:?} {d} units from the seam: brush {brush:.1}, voxel {voxel:.1}, step {step:.3}"
            );
            assert!(brush > 30.0 && voxel > 30.0, "both sides are lit");
            // The textures differ in grain, so the real look is allowed more.
            let allowed = if look == Look::Flat { 0.06 } else { 0.15 };
            assert!(step < allowed, "{look:?} at {d}: {brush} against {voxel}");
        }
    }
}

#[test]
fn a_draft_lights_voxel_floors_and_shadows_them_hard() {
    let level = voxel_scene::shadowed_room();
    let cfg = BuildConfig {
        luxel_size: 0.25,
        shadow_samples: 16,
        ..stability::config()
    };
    let compiled = Compiled::new(&level, &cfg).expect("compiles");
    let draft = Arc::new(compiled.draft().expect("drafts"));
    let draft = compiled
        .into_renderer(draft, stability::materials_for(Look::Flat))
        .expect("assembles");
    let full = build(&level, Look::Flat);
    let view = voxel_scene::rig().view(crawl::Motion::Strafe, 0, 0.25, 1.3, SIZE.1);
    // One floor point the slab leaves lit and one in its shadow.
    let (lit_point, dark_point) = (Vec3::new(16.0, 3.0, 0.0), Vec3::new(19.0, 8.0, 0.0));
    let read = |r: &LevelRenderer| {
        (
            window(r, &view, lit_point, 3),
            window(r, &view, dark_point, 3),
        )
    };
    let ((d_lit, d_dark), (f_lit, f_dark)) = (read(&draft), read(&full));
    println!("draft: lit {d_lit:.1} dark {d_dark:.1}; full: lit {f_lit:.1} dark {f_dark:.1}");
    assert!(
        f_lit > 20.0 && f_dark < 0.2 * f_lit,
        "the bake shadows the floor"
    );
    assert!(
        d_lit > 0.6 * f_lit && d_lit < 1.6 * f_lit,
        "the draft lights it"
    );
    assert!(d_dark < 0.2 * d_lit, "and shadows it");
}
