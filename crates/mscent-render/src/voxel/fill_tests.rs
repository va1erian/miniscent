//! Voxel surfaces drawn through a whole level frame, read back as pixels. Fuchsia
//! sky shows through any hole.

use std::path::Path;
use std::sync::OnceLock;

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab, fill};
use mscent_world::{Brush, Entity, Level, Op};

use crate::level::{BuildConfig, Compiled, LevelRenderer, Quality, View};
use crate::{Framebuffer, MaterialTable};

const HOLE: u32 = 0xffff_00ff;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn cfg() -> BuildConfig {
    BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        sky_top: HOLE,
        sky_bottom: HOLE,
        fog_start: 300.0,
        fog_end: 400.0,
        ..BuildConfig::default()
    }
}

fn light(pos: Vec3) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 1.0, 1.0),
        radius: 30.0,
        intensity: 1.0,
    }
}

fn renderer(level: &Level) -> LevelRenderer {
    let c = Compiled::new(level, &cfg()).expect("compiles");
    let map = std::sync::Arc::new(c.draft().expect("drafts"));
    c.into_renderer(map, materials()).expect("assembles")
}

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// A warped gyroid, positive solid.
fn gyroid(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55) + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7) * 0.18 + 0.25
}

/// A closed gyroid cave in solid rock.
fn gyroid_level() -> Level {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Quarter, [0; 3], [4, 4, 2]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    fill(&mut volume, &mut ctx, gyroid, 4);
    Level {
        volumes: vec![volume],
        entities: vec![light(Vec3::new(8.0, 8.0, 4.0))],
        ..Default::default()
    }
}

fn gyroid_renderer() -> &'static LevelRenderer {
    static R: OnceLock<LevelRenderer> = OnceLock::new();
    R.get_or_init(|| renderer(&gyroid_level()))
}

/// Air positions well inside the box, away from every surface.
fn air_pose(rng: &mut Rng) -> Vec3 {
    loop {
        let p = v(
            rng.range_f64(2.5, 13.5),
            rng.range_f64(2.5, 13.5),
            rng.range_f64(2.0, 6.0),
        );
        if gyroid(p) < -0.45 {
            return p.to_f32();
        }
    }
}

fn holes(fb: &Framebuffer) -> usize {
    fb.pixels().iter().filter(|&&p| p == HOLE).count()
}

#[test]
fn no_sky_pixel_from_inside_a_closed_cave_over_a_thousand_poses() {
    let r = gyroid_renderer();
    let mut rng = Rng::new(11);
    let sizes = [(160, 90), (321, 181), (640, 360)];
    let mut total = 0;
    for i in 0..1000 {
        let (w, h) = sizes[i % sizes.len()];
        let mut fb = Framebuffer::new(w, h).expect("fb");
        let eye = air_pose(&mut rng);
        let yaw = rng.range_f64(-3.1, 3.1) as f32;
        let pitch = rng.range_f64(-1.2, 1.2) as f32;
        let view = View::from_angles(eye, yaw, pitch, 0.0);
        let stats = r
            .render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        assert!(stats.voxel_triangles > 0, "pose {i} drew nothing");
        total += holes(&fb);
        assert_eq!(
            holes(&fb),
            0,
            "pose {i}: eye {eye:?} yaw {yaw} pitch {pitch}"
        );
    }
    assert_eq!(total, 0);
}

#[test]
fn level_of_detail_opens_no_hole_either() {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Eighth, [0; 3], [8, 8, 4]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    fill(&mut volume, &mut ctx, gyroid, 4);
    let level = Level {
        volumes: vec![volume],
        ..Default::default()
    };
    let r = renderer(&level);
    let mut rng = Rng::new(3);
    let mut fb = Framebuffer::new(320, 180).expect("fb");
    for i in 0..60 {
        let eye = air_pose(&mut rng);
        let yaw = rng.range_f64(-3.1, 3.1) as f32;
        let view = View::from_angles(eye, yaw, 0.0, 0.0);
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        assert_eq!(holes(&fb), 0, "pose {i}: eye {eye:?} yaw {yaw}");
    }
}

fn room() -> Brush {
    Brush::cuboid(v(-8.0, -8.0, 0.0), v(8.0, 8.0, 6.0), Op::Subtract, 0)
        .expect("valid")
        .with_material(mscent_world::materials::ROCK)
}

fn room_level(volumes: Vec<Volume>) -> Level {
    Level {
        brushes: vec![room()],
        volumes,
        entities: vec![light(Vec3::new(0.0, 0.0, 4.0))],
        ..Default::default()
    }
}

fn picture(r: &LevelRenderer, eye: Vec3, yaw: f32, pitch: f32) -> Framebuffer {
    let mut fb = Framebuffer::new(320, 180).expect("fb");
    let view = View::from_angles(eye, yaw, pitch, 0.0);
    r.render(&mut fb, &view, Quality::Low, &[], None)
        .expect("renders");
    fb
}

#[test]
fn a_seeded_subtract_volume_leaves_a_room_with_no_hole_and_about_the_same_look() {
    // A box across the +Y wall: half room air, half rock.
    let volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [-16, 16, 0], [2, 2, 1]).expect("valid");
    let plain = renderer(&room_level(Vec::new()));
    let seeded = renderer(&room_level(vec![volume]));
    let eye = Vec3::new(0.0, -4.0, 3.0);
    let (a, b) = (
        picture(&plain, eye, 0.0, 0.0),
        picture(&seeded, eye, 0.0, 0.0),
    );
    assert_eq!(holes(&a), 0);
    assert_eq!(holes(&b), 0, "the seam shows the sky");
    let lum = |p: u32| ((p >> 16) & 255) + ((p >> 8) & 255) + (p & 255);
    let mean = |f: &Framebuffer| {
        f.pixels().iter().map(|&p| u64::from(lum(p))).sum::<u64>() / f.pixels().len() as u64
    };
    let (ma, mb) = (mean(&a) as i64, mean(&b) as i64);
    assert!((ma - mb).abs() < ma / 4 + 8, "mean {ma} against {mb}");
}

#[test]
fn a_volume_with_only_shell_faces_still_renders() {
    // Alone in solid rock: its leaf has no drawn face at all, so only chunk
    // bounds keep the walk from culling it.
    let r = gyroid_renderer();
    assert!(r.scene().faces().is_empty(), "shell faces are never kept");
    let fb = picture(r, air_pose(&mut Rng::new(1)), 0.3, 0.1);
    assert!(fb.pixels().iter().any(|&p| p != HOLE));
}

#[test]
fn seen_from_a_room_a_tunnel_through_the_wall_opens_with_no_sky_in_it() {
    let mut volume =
        Volume::new(Op::Subtract, 1, CellSize::Quarter, [-16, 16, 0], [2, 3, 1]).expect("valid");
    let brushes = vec![room()];
    let mut ctx = Context::new(&volume.clone(), &brushes);
    let tunnel = Prim::round_cone(v(0.0, 7.0, 2.5), v(0.0, 14.0, 2.5), 1.5, 1.5).expect("valid");
    apply_dab(&mut volume, &mut ctx, &tunnel, DabOp::Carve);
    let r = renderer(&room_level(vec![volume]));
    let mut rng = Rng::new(5);
    for i in 0..200 {
        let eye = Vec3::new(
            rng.range_f64(-6.0, 6.0) as f32,
            rng.range_f64(-6.0, 6.0) as f32,
            rng.range_f64(1.0, 5.0) as f32,
        );
        let yaw = rng.range_f64(-3.1, 3.1) as f32;
        let fb = picture(&r, eye, yaw, 0.0);
        assert_eq!(holes(&fb), 0, "pose {i}: eye {eye:?} yaw {yaw}");
    }
}

#[test]
fn a_near_plane_through_the_surface_opens_no_hole() {
    let r = gyroid_renderer();
    let mut rng = Rng::new(17);
    for i in 0..300 {
        // Close to the surface, so triangles beside the eye cross the near plane,
        // but not so close that the plane's corners reach through the wall.
        let eye = loop {
            let p = v(
                rng.range_f64(2.5, 13.5),
                rng.range_f64(2.5, 13.5),
                rng.range_f64(2.0, 6.0),
            );
            if (-0.3..-0.2).contains(&gyroid(p)) {
                break p.to_f32();
            }
        };
        let view = View::from_angles(eye, rng.range_f64(-3.1, 3.1) as f32, 0.2, 0.0);
        let mut fb = Framebuffer::new(160, 90).expect("fb");
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders");
        assert_eq!(holes(&fb), 0, "pose {i}: eye {eye:?}");
    }
}

#[test]
fn every_quality_draws_the_same_coverage_with_different_shading() {
    let r = gyroid_renderer();
    let eye = air_pose(&mut Rng::new(1));
    let view = View::from_angles(eye, 0.4, 0.0, 0.0);
    let mut pics = Vec::new();
    for q in [Quality::Low, Quality::Medium, Quality::High] {
        let mut fb = Framebuffer::new(320, 180).expect("fb");
        r.render(&mut fb, &view, q, &[], None).expect("renders");
        assert_eq!(holes(&fb), 0, "{q:?}");
        pics.push(fb.pixels().to_vec());
    }
    assert_ne!(pics[0], pics[1], "bump changes the picture");
}

#[test]
fn a_frame_is_the_same_on_any_number_of_workers() {
    let level = gyroid_level();
    let mut one = renderer(&level);
    one.set_tiling(24, 1);
    let mut many = renderer(&level);
    many.set_tiling(7, 4);
    let eye = air_pose(&mut Rng::new(1));
    assert_eq!(
        picture(&one, eye, 0.5, 0.1).pixels(),
        picture(&many, eye, 0.5, 0.1).pixels()
    );
}

#[test]
fn an_overloaded_view_stays_near_the_budget_by_losing_distant_detail() {
    let mut volume =
        Volume::new(Op::Subtract, 0, CellSize::Eighth, [0; 3], [12, 12, 4]).expect("valid");
    let mut ctx = Context::new(&volume.clone(), &[]);
    fill(&mut volume, &mut ctx, |p| gyroid(p * 3.0), 4);
    let level = Level {
        volumes: vec![volume],
        ..Default::default()
    };
    let r = renderer(&level);
    let mut rng = Rng::new(9);
    let eye = loop {
        let p = v(
            rng.range_f64(1.0, 3.0),
            rng.range_f64(1.0, 3.0),
            rng.range_f64(2.0, 6.0),
        );
        if gyroid(p * 3.0) < -0.4 {
            break p.to_f32();
        }
    };
    let view = View::from_angles(eye, 0.78, 0.0, 0.0);
    let mut fb = Framebuffer::new(320, 180).expect("fb");
    let first = r
        .render(&mut fb, &view, Quality::Low, &[], None)
        .expect("renders");
    let mut last = first.voxel_triangles;
    for _ in 0..40 {
        last = r
            .render(&mut fb, &view, Quality::Low, &[], None)
            .expect("renders")
            .voxel_triangles;
    }
    println!("first {} last {last}", first.voxel_triangles);
    assert!(
        last <= first.voxel_triangles,
        "{last} after {}",
        first.voxel_triangles
    );
    assert_eq!(holes(&fb), 0);
}

#[test]
fn a_dynamic_light_brightens_a_voxel_wall() {
    let r = gyroid_renderer();
    let eye = air_pose(&mut Rng::new(1));
    let view = View::from_angles(eye, 0.4, 0.0, 0.0);
    let render = |effects: &crate::level::Effects| {
        let mut fb = Framebuffer::new(160, 90).expect("fb");
        r.render_with_effects(&mut fb, &view, Quality::Low, &[], None, effects)
            .expect("renders");
        fb.pixels().iter().map(|&p| u64::from(p & 255)).sum::<u64>()
    };
    let dark = render(&crate::level::Effects::default());
    let mut lamp = crate::DynLight::new();
    assert!(lamp.place(r.scene(), eye, 12.0, Vec3::new(1.0, 1.0, 1.0)));
    let lights = [lamp];
    let lit = render(&crate::level::Effects {
        lights: &lights,
        ..Default::default()
    });
    assert!(lit > dark + dark / 8, "{dark} then {lit}");
}

#[test]
fn remeshed_chunks_open_no_hole_trace_no_ray_and_restore_exactly() {
    use mscent_world::voxel::VoxelWorld;
    let level = gyroid_level();
    let mut r = renderer(&level);
    let map: *const _ = r.lightmap();
    let eye = air_pose(&mut Rng::new(1));
    let view = View::from_angles(eye, 0.4, 0.0, 0.0);
    // Enough frames for the governor to settle: it is free to change the picture
    // of a frame over budget, and that is not what this compares.
    let frame = |r: &LevelRenderer| {
        let mut fb = Framebuffer::new(320, 180).expect("fb");
        for _ in 0..80 {
            r.render(&mut fb, &view, Quality::Low, &[], None)
                .expect("renders");
        }
        fb
    };
    let before = frame(&r);
    let world = r.scene().voxels().world().clone();
    let brushes = level.static_brushes();
    let bsp = r.scene().bsp().clone();
    let mut volumes = level.volumes.clone();
    let shape = volumes[0].clone();
    let bite = Prim::sphere(eye.to_f64() + v(0.0, 2.0, 0.0), 1.5).expect("valid");
    let mut ctx = Context::new(&shape, &brushes);
    let chunks = apply_dab(&mut volumes[0], &mut ctx, &bite, DabOp::Carve);
    assert!(!chunks.is_empty());
    let list = VoxelWorld::remesh(&volumes, &brushes, &bsp, &[(0, chunks)]);
    r.apply_voxels(crate::VoxelChange::Chunks(list));
    assert_eq!(holes(&frame(&r)), 0, "a carve opens onto rock, not the sky");
    assert!(std::ptr::eq(map, r.lightmap()), "the lightmap stays");
    r.apply_voxels(crate::VoxelChange::Restore(Box::new(world)));
    assert_eq!(frame(&r).pixels(), before.pixels(), "restored bit for bit");
}

#[test]
fn a_remade_world_keeps_the_light_of_what_did_not_move() {
    let level = gyroid_level();
    let mut r = renderer(&level);
    let eye = air_pose(&mut Rng::new(1));
    let before = picture(&r, eye, 0.4, 0.0);
    let world = r.scene().voxels().world().clone();
    r.apply_voxels(crate::VoxelChange::All(Box::new(world)));
    assert_eq!(picture(&r, eye, 0.4, 0.0).pixels(), before.pixels());
}
