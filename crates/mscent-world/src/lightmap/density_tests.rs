//! Per-face luxel density: coarse faces, faces with no luxels, and what the
//! store makes of them.

use super::atlas::{AtlasError, Rect};
use super::store::{self, StoreError, input_key};
use super::uv::project_face;
use super::*;
use crate::brush::{Brush, Op};
use crate::bsp::Bsp;
use crate::csg::compile;
use mscent_math::{Vec3, Vec3d};

/// Bytes of one face record in a stored lightmap, and where the first starts:
/// after the header, the atlas size and the face count.
const FACE_BYTES: usize = 76;
const FACE_AT: usize = 26;
/// Offset of a rectangle's width in a face record.
const RECT_W: usize = 8;

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn room() -> Bsp {
    let (lo, hi) = (Vec3d::new(0.0, 0.0, 0.0), Vec3d::new(100.0, 100.0, 100.0));
    compile(&[Brush::cuboid(lo, hi, Op::Subtract, 0).unwrap()]).unwrap()
}

/// A square of `side` on the plane `z = at`, facing up, with `density`.
fn square(at: f32, side: f32, density: LuxelDensity) -> LightFace {
    let p = |x: f32, y: f32| v3(x, y, at);
    LightFace {
        vertices: vec![p(0.0, 0.0), p(side, 0.0), p(side, side), p(0.0, side)],
        normal: v3(0.0, 0.0, 1.0),
        density,
    }
}

fn lamp() -> PointLight {
    PointLight {
        position: v3(30.0, 40.0, 60.0),
        color: v3(1.0, 1.0, 1.0),
        radius: 200.0,
        cone: None,
    }
}

fn cfg(luxel_size: f32) -> BakeConfig {
    BakeConfig {
        luxel_size,
        workers: 2,
        ..BakeConfig::default()
    }
}

fn luxels_of(map: &Lightmap, face: usize) -> Vec<Luxel> {
    let p = &map.projections()[face];
    let at = |x, y| map.face_luxel(face, x, y).expect("inside");
    let cells = (0..p.height).flat_map(|y| (0..p.width).map(move |x| (x, y)));
    cells.map(|(x, y)| at(x, y)).collect()
}

#[test]
fn a_coarse_face_bakes_at_a_multiple_and_leaves_the_others_as_they_were() {
    let full = square(0.0, 100.0, LuxelDensity::Full);
    let coarse = square(50.0, 100.0, LuxelDensity::Coarse(4.0));
    let both = [full.clone(), coarse];
    // No bounce, so the second face is not a light for the first.
    let direct = BakeConfig {
        albedo: 0.0,
        ..cfg(2.0)
    };
    let alone = bake(&room(), &[full], &[lamp()], &direct).unwrap();
    let pair = bake(&room(), &both, &[lamp()], &direct).unwrap();
    assert_eq!(pair.projections()[0], alone.projections()[0]);
    assert_eq!(pair.projections()[1].luxel_size, 8.0);
    assert_eq!(pair.projections()[1].width, 13);
    assert_eq!(pair.finest_luxel(), Some(2.0));
    assert_eq!(luxels_of(&pair, 0), luxels_of(&alone, 0));
}

#[test]
fn a_factor_that_is_not_a_sane_factor_cannot_break_the_grid() {
    for k in [f32::NAN, -3.0, 0.0, 0.5, f32::INFINITY, 1e30] {
        let faces = [square(0.0, 100.0, LuxelDensity::Coarse(k))];
        let map = bake(&room(), &faces, &[lamp()], &cfg(2.0)).unwrap();
        let size = map.projections()[0].luxel_size;
        assert!(size.is_finite() && size >= 2.0, "{k}: {size}");
        assert!(Lightmap::from_bytes(&map.to_bytes(7), 7).is_ok(), "{k}");
    }
}

#[test]
fn coarse_works_with_the_growth_for_huge_faces_and_for_a_full_atlas() {
    // 1000 units at 0.125 is past the grid cap, and a coarse face hits it too.
    let huge = square(0.0, 1000.0, LuxelDensity::Coarse(8.0));
    let p = project_face(&huge, 0, 0.125 * 8.0).unwrap();
    assert_eq!((p.width, p.height), (128, 128));
    let faces = [huge, square(1.0, 32.0, LuxelDensity::Coarse(4.0))];
    let bake_of = |faces: &[LightFace]| bake(&room(), faces, &[lamp()], &cfg(0.125)).unwrap();
    let map = bake_of(&faces);
    assert_eq!(map.projections()[0].width, 128);
    assert_eq!(map.projections()[1].luxel_size, 0.5);
    // A tiny atlas coarsens every face; the coarse one stays the coarser.
    let faces = [
        square(0.0, 32.0, LuxelDensity::Full),
        square(1.0, 32.0, LuxelDensity::Coarse(4.0)),
    ];
    let small = BakeConfig {
        atlas_width: 20,
        atlas_max_height: 20,
        ..cfg(0.125)
    };
    let map = bake(&room(), &faces, &[lamp()], &small).unwrap();
    let (a, b) = (&map.projections()[0], &map.projections()[1]);
    assert!(b.luxel_size >= a.luxel_size * 3.99, "{a:?} {b:?}");
    assert!(map.atlas().width() == 20 && map.atlas().height() <= 20);
}

#[test]
fn a_face_with_no_luxels_keeps_its_place_and_costs_nothing() {
    let floor = square(0.0, 100.0, LuxelDensity::Full);
    let wall = square(10.0, 60.0, LuxelDensity::Full);
    let sky = square(50.0, 100.0, LuxelDensity::None);
    let lamps = [lamp()];
    let without = bake(&room(), &[floor.clone(), wall.clone()], &lamps, &cfg(2.0)).unwrap();
    let with = bake(&room(), &[floor, sky, wall], &lamps, &cfg(2.0)).unwrap();
    assert_eq!(with.projections().len(), 3, "every face has an entry");
    assert_eq!(with.atlas().rects()[1], Rect::EMPTY);
    assert!(with.face_grid(1).is_none());
    assert!(with.face_luxel(1, 0, 0).is_none());
    assert!(with.sample_world(1, v3(5.0, 5.0, 50.0)).is_none());
    assert_eq!(with.finest_luxel(), without.finest_luxel());
    assert_eq!(with.atlas().height(), without.atlas().height());
    // Bounce is on: nothing leaks from it, and the later face is just renumbered.
    assert_eq!(luxels_of(&with, 0), luxels_of(&without, 0));
    assert_eq!(luxels_of(&with, 2), luxels_of(&without, 1));
}

#[test]
fn a_bake_of_nothing_but_faces_without_luxels_stores_and_reloads() {
    let faces = [
        square(0.0, 10.0, LuxelDensity::None),
        square(5.0, 10.0, LuxelDensity::None),
    ];
    let map = bake(&room(), &faces, &[lamp()], &cfg(1.0)).unwrap();
    let key = input_key(&faces, &[lamp()], &cfg(1.0), 0);
    let back = Lightmap::from_bytes(&map.to_bytes(key), key).unwrap();
    assert_eq!(back.atlas(), map.atlas());
    assert!(back.face_grid(0).is_none() && back.face_grid(1).is_none());
}

#[test]
fn faces_without_luxels_do_not_stop_the_atlas_giving_up() {
    let mut faces = vec![square(0.0, 8.0, LuxelDensity::None)];
    faces.extend((0..3).map(|i| square(i as f32, 8.0, LuxelDensity::Full)));
    let tiny = BakeConfig {
        atlas_width: 3,
        atlas_max_height: 6,
        ..cfg(0.125)
    };
    let err = bake(&room(), &faces, &[], &tiny).err();
    assert_eq!(err, Some(LightmapError::Atlas(AtlasError::TooTall)));
}

/// The key as it was made before densities existed.
fn legacy_key(faces: &[LightFace], lights: &[PointLight], cfg: &BakeConfig) -> u64 {
    let mut h = store::Fnv(store::FNV_BASIS);
    h.bytes(&store::MAGIC);
    h.u32(u32::from(store::VERSION));
    let first = [
        cfg.luxel_size,
        cfg.normal_bias,
        cfg.light_radius,
        cfg.albedo,
        cfg.bounce_gain,
    ];
    first.into_iter().for_each(|v| h.f32(v));
    h.vec3(cfg.ambient);
    for v in [cfg.shadow_samples, cfg.atlas_width, cfg.atlas_max_height] {
        h.u32(v);
    }
    h.u32(lights.len() as u32);
    for l in lights {
        h.vec3(l.position);
        h.vec3(l.color);
        h.f32(l.radius);
    }
    h.u32(faces.len() as u32);
    for f in faces {
        h.u32(f.vertices.len() as u32);
        f.vertices.iter().for_each(|&v| h.vec3(v));
        h.vec3(f.normal);
    }
    h.bytes(&0u64.to_le_bytes());
    h.0
}

#[test]
fn full_density_leaves_every_old_key_as_it_was_and_others_change_it() {
    let faces = [
        square(0.0, 10.0, LuxelDensity::Full),
        square(5.0, 10.0, LuxelDensity::Full),
    ];
    let c = cfg(0.5);
    let old = legacy_key(&faces, &[lamp()], &c);
    assert_eq!(input_key(&faces, &[lamp()], &c, 0), old);
    let key = |density| {
        let second = LightFace {
            density,
            ..faces[1].clone()
        };
        input_key(&[faces[0].clone(), second], &[lamp()], &c, 0)
    };
    let all = [
        old,
        key(LuxelDensity::Coarse(32.0)),
        key(LuxelDensity::Coarse(16.0)),
        key(LuxelDensity::None),
    ];
    for (i, a) in all.iter().enumerate() {
        assert!(all.iter().skip(i + 1).all(|b| a != b), "{all:?}");
    }
    let nan = key(LuxelDensity::Coarse(f32::NAN));
    assert_eq!(nan, key(LuxelDensity::Coarse(1.0)), "same bake, same key");
}

fn stored(faces: &[LightFace]) -> (Vec<u8>, u64) {
    let key = input_key(faces, &[lamp()], &cfg(2.0), 0);
    let map = bake(&room(), faces, &[lamp()], &cfg(2.0)).unwrap();
    (map.to_bytes(key), key)
}

/// Recomputes the trailing checksum, so a test reaches the field checks.
fn reseal(b: &mut [u8]) {
    let n = b.len() - 8;
    let mut h = store::Fnv(store::FNV_BASIS);
    h.bytes(&b[..n]);
    b[n..].copy_from_slice(&h.0.to_le_bytes());
}

#[test]
fn a_mixed_bake_round_trips_byte_for_byte() {
    let faces = [
        square(0.0, 100.0, LuxelDensity::Full),
        square(10.0, 50.0, LuxelDensity::None),
        square(20.0, 100.0, LuxelDensity::Coarse(32.0)),
    ];
    let (bytes, key) = stored(&faces);
    let back = Lightmap::from_bytes(&bytes, key).unwrap();
    assert_eq!(back.to_bytes(key), bytes);
    assert_eq!(back.atlas().rects()[1], Rect::EMPTY);
}

#[test]
fn a_forged_rectangle_is_refused_unless_it_is_exactly_the_empty_one() {
    let faces = [
        square(0.0, 100.0, LuxelDensity::Full),
        square(10.0, 50.0, LuxelDensity::None),
    ];
    let (bytes, key) = stored(&faces);
    let second = FACE_AT + FACE_BYTES;
    let patch = |at: usize, v: u32| {
        let mut b = bytes.clone();
        b[at..at + 4].copy_from_slice(&v.to_le_bytes());
        reseal(&mut b);
        Lightmap::from_bytes(&b, key)
    };
    assert!(Lightmap::from_bytes(&bytes, key).is_ok());
    // A real face made hollow: its width gone, its height kept.
    assert_eq!(patch(FACE_AT + RECT_W, 0).err(), Some(StoreError::Corrupt));
    // The empty rectangle given a position, or only one side.
    assert_eq!(patch(second, 3).err(), Some(StoreError::Corrupt));
    assert_eq!(patch(second + RECT_W, 4).err(), Some(StoreError::Corrupt));
    // A real face emptied entirely, with the checksum fixed: nothing in a level
    // can tell, but the face has no map, and nothing panics.
    let mut emptied = bytes.clone();
    emptied[FACE_AT..FACE_AT + 16].fill(0);
    reseal(&mut emptied);
    let map = Lightmap::from_bytes(&emptied, key).unwrap();
    assert!(map.face_grid(0).is_none());
    assert!(map.sample_world(0, v3(1.0, 1.0, 0.0)).is_none());
}
