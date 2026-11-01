use super::ray::segment_clear;
use super::uv::project_face;
use super::*;
use crate::brush::{Brush, Op};
use crate::bsp::Bsp;
use crate::csg::compile;
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn cuboid(lo: [f64; 3], hi: [f64; 3], op: Op, order: u32) -> Brush {
    let (a, b) = (
        Vec3d::new(lo[0], lo[1], lo[2]),
        Vec3d::new(hi[0], hi[1], hi[2]),
    );
    Brush::cuboid(a, b, op, order).unwrap()
}

fn build(brushes: &[Brush]) -> Bsp {
    compile(brushes).unwrap()
}

/// Axis-aligned quad on the plane `axis = at`, with a given normal sign.
fn quad(axis: usize, at: f32, a: (f32, f32), b: (f32, f32), sign: f32) -> LightFace {
    let pt = |u: f32, w: f32| match axis {
        0 => v3(at, u, w),
        1 => v3(u, at, w),
        _ => v3(u, w, at),
    };
    let mut n = [0.0; 3];
    n[axis] = sign;
    LightFace {
        vertices: vec![pt(a.0, b.0), pt(a.1, b.0), pt(a.1, b.1), pt(a.0, b.1)],
        normal: v3(n[0], n[1], n[2]),
    }
}

fn cfg() -> BakeConfig {
    BakeConfig {
        luxel_size: 10.0,
        workers: 2,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        ..BakeConfig::default()
    }
}

fn white_light(pos: Vec3, radius: f32) -> PointLight {
    PointLight {
        position: pos,
        color: v3(1.0, 1.0, 1.0),
        radius,
    }
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-4, "{a} vs {b}");
}

fn room() -> Brush {
    cuboid([0.0; 3], [100.0; 3], Op::Subtract, 0)
}

fn floor() -> LightFace {
    quad(2, 0.0, (0.0, 100.0), (0.0, 100.0), 1.0)
}

#[test]
fn projection_frame_is_orthonormal_and_covers_face() {
    let f = quad(0, 5.0, (0.0, 95.0), (0.0, 33.0), -1.0);
    let p = project_face(&f, 0, 10.0).unwrap();
    close(p.tangent.length(), 1.0);
    close(p.bitangent.length(), 1.0);
    close(p.tangent.dot(p.bitangent), 0.0);
    close(p.tangent.cross(p.bitangent).dot(p.normal), 1.0);
    assert_eq!(p.width.max(p.height), 10);
    assert_eq!(p.width.min(p.height), 4);
    for x in 0..p.width {
        for y in 0..p.height {
            close(p.luxel_center(x, y).x, 5.0);
        }
    }
}

#[test]
fn huge_faces_get_coarser_luxels() {
    let f = quad(2, 0.0, (0.0, 100000.0), (0.0, 10.0), 1.0);
    let p = project_face(&f, 0, 1.0).unwrap();
    assert!(p.width <= MAX_FACE_LUXELS && p.luxel_size > 1.0);
}

#[test]
fn degenerate_and_hostile_faces_error() {
    let mut f = floor();
    f.vertices.truncate(2);
    assert_eq!(
        project_face(&f, 3, 8.0),
        Err(LightmapError::DegenerateFace(3))
    );
    let mut f = floor();
    f.vertices[1] = v3(f32::NAN, 0.0, 0.0);
    assert_eq!(
        project_face(&f, 1, 8.0),
        Err(LightmapError::NonFiniteFace(1))
    );
    let mut f = floor();
    f.normal = Vec3::ZERO;
    assert!(project_face(&f, 0, 8.0).is_err());
    let line = LightFace {
        vertices: vec![v3(0.0, 0.0, 0.0); 3],
        normal: v3(0.0, 0.0, 1.0),
    };
    assert!(project_face(&line, 0, 8.0).is_err());
}

#[test]
fn atlas_has_no_overlap_and_keeps_gutters_apart() {
    let mut rng = Rng::new(7);
    for _ in 0..50 {
        let n = 1 + rng.below(40) as usize;
        let sizes: Vec<(u32, u32)> = (0..n)
            .map(|_| (1 + rng.below(30), 1 + rng.below(30)))
            .collect();
        let atlas = Atlas::pack(&sizes, 128, 4096).unwrap();
        let r = atlas.rects();
        for (i, a) in r.iter().enumerate() {
            assert_eq!((a.w, a.h), sizes[i]);
            assert!(a.x >= PADDING && a.x + a.w + PADDING <= atlas.width());
            assert!(a.y >= PADDING && a.y + a.h + PADDING <= atlas.height());
            for b in &r[i + 1..] {
                // Inflating one by the gutter on each side must still miss.
                let apart = a.x + a.w + 2 * PADDING <= b.x
                    || b.x + b.w + 2 * PADDING <= a.x
                    || a.y + a.h + 2 * PADDING <= b.y
                    || b.y + b.h + 2 * PADDING <= a.y;
                assert!(apart, "{a:?} touches {b:?}");
            }
        }
    }
}

#[test]
fn atlas_rejects_bad_input_and_dumps_ppm() {
    assert_eq!(Atlas::pack(&[(0, 4)], 64, 64), Err(AtlasError::BadSize(0)));
    assert_eq!(Atlas::pack(&[(63, 4)], 64, 64), Err(AtlasError::TooWide(0)));
    assert_eq!(
        Atlas::pack(&[(30, 30), (30, 30)], 32, 40),
        Err(AtlasError::TooTall)
    );
    assert!(Atlas::pack(&[(u32::MAX, 1)], 64, 64).is_err());
    let atlas = Atlas::pack(&[(4, 4), (2, 3)], 16, 16).unwrap();
    let dump = atlas.debug_ppm();
    let head = format!("P6\n{} {}\n255\n", atlas.width(), atlas.height());
    assert!(dump.starts_with(head.as_bytes()));
    assert_eq!(
        dump.len(),
        head.len() + (atlas.width() * atlas.height() * 3) as usize
    );
}

#[test]
fn segment_clear_respects_walls_and_thin_slabs() {
    let bsp = build(&[
        room(),
        cuboid([40.0, 0.0, 0.0], [41.0, 100.0, 100.0], Op::Add, 1),
    ]);
    let d = |x: f64, y: f64, z: f64| Vec3d::new(x, y, z);
    assert!(segment_clear(
        &bsp,
        d(10.0, 50.0, 50.0),
        d(30.0, 50.0, 80.0)
    ));
    assert!(!segment_clear(
        &bsp,
        d(10.0, 50.0, 50.0),
        d(90.0, 50.0, 50.0)
    ));
    assert!(!segment_clear(
        &bsp,
        d(10.0, 50.0, 50.0),
        d(200.0, 50.0, 50.0)
    ));
    assert!(!segment_clear(
        &bsp,
        d(f64::NAN, 0.0, 0.0),
        d(1.0, 1.0, 1.0)
    ));
}

#[test]
fn unshadowed_floor_matches_hand_value() {
    let bsp = build(&[room()]);
    // Light 50 above luxel centre (45, 45): facing 1, falloff (1 - 50/200)^3.
    let light = white_light(v3(45.0, 45.0, 50.0), 200.0);
    let lm = bake(&bsp, &[floor()], &[light], &cfg()).unwrap();
    let l = lm.sample_world(0, v3(45.0, 45.0, 0.0)).unwrap();
    close(l.color.x, 0.421_875);
    close(l.color.y, 0.421_875);
    let p = &lm.projections()[0];
    let dir = l.dir;
    close(dir.x, 0.0);
    close(dir.y, 0.0);
    close(dir.z, 1.0);
    assert_eq!(p.normal, v3(0.0, 0.0, 1.0));
    // A luxel out of the light's range stays black, with the fallback.
    let far = bake(
        &bsp,
        &[floor()],
        &[white_light(v3(45.0, 45.0, 50.0), 60.0)],
        &cfg(),
    )
    .unwrap();
    let dark = far.sample_world(0, v3(95.0, 95.0, 0.0)).unwrap();
    assert_eq!(dark, Luxel::DARK);
}

#[test]
fn plate_casts_the_hand_computed_shadow() {
    // A 20x20 plate at z 20..22 under a light at (50,50,50). A floor point
    // (x,y) is shadowed when the ray crosses the plate: with dx = |x - 50|,
    // that is dx * 28/50 < 10 (top face, the tightest) and likewise for dy.
    let plate = cuboid([40.0, 40.0, 20.0], [60.0, 60.0, 22.0], Op::Add, 1);
    let bsp = build(&[room(), plate]);
    let light = white_light(v3(50.0, 50.0, 50.0), 200.0);
    let lm = bake(&bsp, &[floor()], &[light], &cfg()).unwrap();
    let at = |x: f32, y: f32| lm.sample_world(0, v3(x, y, 0.0)).unwrap().color.x;
    assert_eq!(at(55.0, 55.0), 0.0);
    assert_eq!(at(65.0, 55.0), 0.0); // dx 15 * 0.56 = 8.4 < 10
    assert_eq!(at(45.0, 45.0), 0.0);
    // dx 25 * 0.56 = 14 > 10: lit. d^2 = 625 + 625 + 2500 (cubic falloff).
    close(at(75.0, 75.0), 0.272_698_74);
    close(at(75.0, 55.0), at(55.0, 75.0));
    assert!(at(75.0, 55.0) > 0.0);
    assert!(at(5.0, 5.0) > 0.0);
}

#[test]
fn directions_are_unit_and_point_at_the_light() {
    let bsp = build(&[room()]);
    let light = white_light(v3(20.0, 70.0, 40.0), 150.0);
    let lm = bake(&bsp, &[floor()], &[light], &cfg()).unwrap();
    let p = lm.projections()[0];
    for y in 0..p.height {
        for x in 0..p.width {
            let l = lm.face_luxel(0, x, y).unwrap();
            close(l.dir.length(), 1.0);
            let world = p.tangent * l.dir.x + p.bitangent * l.dir.y + p.normal * l.dir.z;
            let want = (light.position - p.luxel_center(x, y))
                .normalized()
                .unwrap();
            if l.color.x > 0.0 {
                assert!(world.dot(want) > 0.9999, "luxel {x},{y}");
            }
        }
    }
}

#[test]
fn lightless_luxels_use_the_fallback_direction() {
    let bsp = build(&[room()]);
    let lm = bake(&bsp, &[floor()], &[], &cfg()).unwrap();
    let again = bake(&bsp, &[floor()], &[], &cfg()).unwrap();
    for y in 0..lm.projections()[0].height {
        let l = lm.face_luxel(0, 0, y).unwrap();
        assert_eq!(l, Luxel::DARK);
        assert_eq!(l, again.face_luxel(0, 0, y).unwrap());
    }
}

#[test]
fn worker_count_does_not_change_the_result() {
    let bsp = build(&[room()]);
    let lights = [white_light(v3(30.0, 30.0, 60.0), 200.0)];
    let faces = [floor(), quad(0, 0.0, (0.0, 100.0), (0.0, 100.0), 1.0)];
    let one = BakeConfig {
        workers: 1,
        albedo: 0.5,
        ..cfg()
    };
    let many = BakeConfig {
        workers: 5,
        albedo: 0.5,
        ..cfg()
    };
    let a = bake(&bsp, &faces, &lights, &one).unwrap();
    let b = bake(&bsp, &faces, &lights, &many).unwrap();
    assert_eq!(a.color_ppm(), b.color_ppm());
    assert_eq!(a.texel(3, 3), b.texel(3, 3));
}

#[test]
fn gutter_repeats_the_edge_luxel() {
    let bsp = build(&[room()]);
    let lm = bake(
        &bsp,
        &[floor()],
        &[white_light(v3(10.0, 10.0, 30.0), 200.0)],
        &cfg(),
    )
    .unwrap();
    let r = lm.atlas().rects()[0];
    assert_eq!(lm.texel(r.x - 1, r.y), lm.texel(r.x, r.y));
    assert_eq!(lm.texel(r.x - 1, r.y - 1), lm.texel(r.x, r.y));
    assert_eq!(
        lm.texel(r.x + r.w, r.y + r.h),
        lm.texel(r.x + r.w - 1, r.y + r.h - 1)
    );
    assert_eq!(lm.texel(lm.atlas().width(), 0), None);
}

#[test]
fn invalid_inputs_are_errors() {
    let bsp = build(&[room()]);
    let mut bad = white_light(Vec3::ZERO, 0.0);
    assert_eq!(
        bake(&bsp, &[floor()], &[bad], &cfg()).err(),
        Some(LightmapError::InvalidLight(0))
    );
    bad.radius = 10.0;
    bad.position = v3(f32::INFINITY, 0.0, 0.0);
    assert!(bake(&bsp, &[floor()], &[bad], &cfg()).is_err());
    let c = BakeConfig {
        luxel_size: 0.0,
        ..cfg()
    };
    assert_eq!(
        bake(&bsp, &[floor()], &[], &c).err(),
        Some(LightmapError::InvalidConfig)
    );
}

/// Two rooms joined by a low door. The lamp hangs in room A under a lintel
/// that hides it from the far wall of room B.
fn two_rooms() -> (Bsp, Vec<LightFace>, PointLight) {
    let bsp = build(&[
        cuboid([0.0, 0.0, 0.0], [96.0, 100.0, 100.0], Op::Subtract, 0),
        cuboid([104.0, 0.0, 0.0], [200.0, 100.0, 100.0], Op::Subtract, 1),
        cuboid([90.0, 40.0, 0.0], [110.0, 60.0, 40.0], Op::Subtract, 2),
    ]);
    let floor_a = quad(2, 0.0, (0.0, 96.0), (0.0, 100.0), 1.0);
    let wall_b = quad(0, 200.0, (0.0, 100.0), (0.0, 100.0), -1.0);
    (
        bsp,
        vec![floor_a, wall_b],
        white_light(v3(50.0, 50.0, 60.0), 300.0),
    )
}

#[test]
fn bounce_lights_a_room_seen_only_through_a_door() {
    let (bsp, faces, light) = two_rooms();
    let probe = v3(200.0, 45.0, 15.0);
    let flat = bake(&bsp, &faces, &[light], &cfg()).unwrap();
    let direct = flat.sample_world(1, probe).unwrap();
    assert_eq!(
        direct.color,
        Vec3::ZERO,
        "probe must be shadowed from the lamp"
    );
    assert_eq!(direct.dir, Luxel::FALLBACK_DIR);

    let bounced = BakeConfig {
        albedo: 0.5,
        ..cfg()
    };
    let lm = bake(&bsp, &faces, &[light], &bounced).unwrap();
    let l = lm.sample_world(1, probe).unwrap();
    assert!(l.color.x > 0.001, "bounce light {:?}", l.color);
    close(l.dir.length(), 1.0);
    // Bounce light arrives from the door side, not along the wall normal.
    assert!(l.dir.z < 1.0);
}

#[test]
fn ambient_fills_unlit_luxels() {
    let (bsp, faces, light) = two_rooms();
    let c = BakeConfig {
        ambient: v3(0.1, 0.2, 0.3),
        ..cfg()
    };
    let lm = bake(&bsp, &faces, &[light], &c).unwrap();
    let l = lm.sample_world(1, v3(200.0, 45.0, 15.0)).unwrap();
    assert_eq!(l.color, v3(0.1, 0.2, 0.3));
}

#[test]
fn face_adapter_and_dumps_work_on_a_real_level() {
    let brushes = [room()];
    let bsp = build(&brushes);
    let faces: Vec<LightFace> = crate::faces::extract_faces(&bsp, &brushes)
        .iter()
        .map(LightFace::from)
        .collect();
    assert_eq!(faces.len(), 6);
    let light = white_light(v3(50.0, 50.0, 50.0), 200.0);
    let lm = bake(&bsp, &faces, &[light], &BakeConfig::default()).unwrap();
    assert!(lm.color_ppm().starts_with(b"P6\n"));
    for (i, f) in faces.iter().enumerate() {
        let c = f.vertices.iter().fold(Vec3::ZERO, |a, &v| a + v) * (1.0 / 4.0);
        assert!(lm.sample_world(i, c).unwrap().color.x > 0.0, "face {i}");
    }
}
