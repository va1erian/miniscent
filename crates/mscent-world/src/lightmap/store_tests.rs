use super::store::{StoreError, input_key};
use super::*;
use crate::brush::{Brush, Op};
use crate::csg::compile;
use mscent_math::{Vec3, Vec3d};

/// The key of a level with no volumes.
fn key(faces: &[LightFace], lights: &[PointLight], cfg: &BakeConfig) -> u64 {
    input_key(faces, lights, cfg, 0)
}

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn room() -> crate::bsp::Bsp {
    let lo = Vec3d::new(0.0, 0.0, 0.0);
    let hi = Vec3d::new(40.0, 40.0, 40.0);
    compile(&[Brush::cuboid(lo, hi, Op::Subtract, 0).unwrap()]).unwrap()
}

fn floor() -> LightFace {
    LightFace {
        vertices: vec![
            v3(0.0, 0.0, 0.0),
            v3(40.0, 0.0, 0.0),
            v3(40.0, 40.0, 0.0),
            v3(0.0, 40.0, 0.0),
        ],
        normal: v3(0.0, 0.0, 1.0),
        density: Default::default(),
    }
}

fn lamp(x: f32) -> PointLight {
    PointLight {
        position: v3(x, 20.0, 30.0),
        color: v3(1.0, 0.9, 0.8),
        radius: 80.0,
    }
}

fn cfg() -> BakeConfig {
    BakeConfig {
        luxel_size: 4.0,
        workers: 2,
        ..BakeConfig::default()
    }
}

fn baked() -> (Lightmap, u64) {
    let (faces, lights) = (vec![floor()], vec![lamp(10.0)]);
    let key = key(&faces, &lights, &cfg());
    (bake(&room(), &faces, &lights, &cfg()).unwrap(), key)
}

#[test]
fn round_trip_reproduces_every_byte() {
    let (map, key) = baked();
    let bytes = map.to_bytes(key);
    let back = Lightmap::from_bytes(&bytes, key).unwrap();
    assert_eq!(back.to_bytes(key), bytes);
    assert_eq!(back.atlas(), map.atlas());
    assert_eq!(back.face_luxel(0, 3, 3), map.face_luxel(0, 3, 3));
}

#[test]
fn key_follows_inputs_but_not_worker_count() {
    let (faces, lights) = (vec![floor()], vec![lamp(10.0)]);
    let base = key(&faces, &lights, &cfg());
    let moved = key(&faces, &[lamp(10.5)], &cfg());
    assert_ne!(base, moved, "a moved light must invalidate");
    let finer = BakeConfig {
        luxel_size: 2.0,
        ..cfg()
    };
    assert_ne!(base, key(&faces, &lights, &finer));
    let softer = BakeConfig {
        shadow_samples: 9,
        ..cfg()
    };
    assert_ne!(base, key(&faces, &lights, &softer));
    let mut taller = floor();
    taller.vertices[0].z = 1.0;
    assert_ne!(base, key(&[taller], &lights, &cfg()));
    let more_workers = BakeConfig {
        workers: 7,
        ..cfg()
    };
    assert_eq!(base, key(&faces, &lights, &more_workers));
    assert_eq!(base, key(&faces, &lights, &cfg()), "stable");
}

#[test]
fn wrong_key_magic_and_version_are_refused() {
    let (map, key) = baked();
    let bytes = map.to_bytes(key);
    assert_eq!(
        Lightmap::from_bytes(&bytes, key ^ 1).err(),
        Some(StoreError::KeyMismatch)
    );
    let mut bad = bytes.clone();
    bad[0] = b'X';
    reseal(&mut bad);
    assert_eq!(
        Lightmap::from_bytes(&bad, key).err(),
        Some(StoreError::BadMagic)
    );
    let mut old = bytes;
    old[4] = 99;
    reseal(&mut old);
    assert_eq!(
        Lightmap::from_bytes(&old, key).err(),
        Some(StoreError::UnsupportedVersion(99))
    );
}

#[test]
fn every_truncation_is_an_error() {
    let (map, key) = baked();
    let bytes = map.to_bytes(key);
    for len in (0..bytes.len()).step_by(7).chain([bytes.len() - 1]) {
        assert!(
            Lightmap::from_bytes(&bytes[..len], key).is_err(),
            "len {len}"
        );
    }
}

/// Recompute the trailing checksum, so a test reaches the field checks.
fn reseal(b: &mut [u8]) {
    let n = b.len() - 8;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in &b[..n] {
        h = (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    b[n..].copy_from_slice(&h.to_le_bytes());
}

#[test]
fn a_flipped_luxel_bit_fails_the_checksum() {
    let (map, key) = baked();
    let mut bytes = map.to_bytes(key);
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0x10;
    assert_eq!(
        Lightmap::from_bytes(&bytes, key).err(),
        Some(StoreError::Corrupt)
    );
}

#[test]
fn hostile_fields_are_errors_not_panics() {
    let (map, key) = baked();
    let bytes = map.to_bytes(key);
    // Face count, atlas size and a NaN luxel size, each overwritten.
    let patch = |at: usize, value: [u8; 4]| {
        let mut b = bytes.clone();
        b[at..at + 4].copy_from_slice(&value);
        reseal(&mut b);
        Lightmap::from_bytes(&b, key)
    };
    assert!(patch(22, u32::MAX.to_le_bytes()).is_err(), "face count");
    assert!(patch(14, u32::MAX.to_le_bytes()).is_err(), "atlas width");
    assert!(patch(18, 0u32.to_le_bytes()).is_err(), "atlas height");
    let luxel_at = 26 + 16 + 4 * 12 + 2 * 4;
    assert!(patch(luxel_at, f32::NAN.to_le_bytes()).is_err(), "NaN size");
    assert!(patch(luxel_at, 0f32.to_le_bytes()).is_err(), "zero size");
    // A rectangle that leaves the atlas.
    assert!(patch(26, 9999u32.to_le_bytes()).is_err(), "rect x");
}

#[test]
fn random_damage_never_panics() {
    let (map, key) = baked();
    let bytes = map.to_bytes(key);
    let mut rng = mscent_math::rng::Rng::new(5);
    for _ in 0..300 {
        let mut b = bytes.clone();
        for _ in 0..4 {
            let i = (rng.next_u64() as usize) % b.len();
            b[i] = rng.next_u64() as u8;
        }
        let _ = Lightmap::from_bytes(&b, key);
    }
}

/// Width, in luxels, of the penumbra at the edge of a pillar's shadow.
fn penumbra_luxels(radius: f32, samples: u32) -> usize {
    let lo = Vec3d::new(0.0, 0.0, 0.0);
    let hi = Vec3d::new(100.0, 100.0, 40.0);
    let pillar_lo = Vec3d::new(40.0, 40.0, 0.0);
    let pillar_hi = Vec3d::new(50.0, 50.0, 40.0);
    let bsp = compile(&[
        Brush::cuboid(lo, hi, Op::Subtract, 0).unwrap(),
        Brush::cuboid(pillar_lo, pillar_hi, Op::Add, 1).unwrap(),
    ])
    .unwrap();
    let floor = LightFace {
        vertices: vec![
            v3(0.0, 0.0, 0.0),
            v3(100.0, 0.0, 0.0),
            v3(100.0, 100.0, 0.0),
            v3(0.0, 100.0, 0.0),
        ],
        normal: v3(0.0, 0.0, 1.0),
        density: Default::default(),
    };
    let lamp = PointLight {
        position: v3(20.0, 45.0, 30.0),
        color: v3(1.0, 1.0, 1.0),
        radius: 400.0,
    };
    let cfg = BakeConfig {
        luxel_size: 1.0,
        workers: 2,
        ambient: Vec3::ZERO,
        albedo: 0.0,
        light_radius: radius,
        shadow_samples: samples,
        ..BakeConfig::default()
    };
    let map = bake(&bsp, &[floor], &[lamp], &cfg).unwrap();
    // A column at x = 80 runs from lit floor into the pillar's shadow,
    // whose edge is at y = 30.
    let col: Vec<f32> = (10..50)
        .map(|y| {
            map.sample_world(0, v3(80.0, y as f32 + 0.5, 0.0))
                .unwrap()
                .color
                .x
        })
        .collect();
    let (lit, dark) = (col[0], col[col.len() - 1]);
    assert!(dark < lit * 0.2, "column {col:?}");
    // Luxels strictly between lit and dark: the width of the penumbra.
    col.iter()
        .filter(|&&v| v > dark + 0.1 * (lit - dark) && v < lit - 0.1 * (lit - dark))
        .count()
}

#[test]
fn a_lamp_disc_makes_a_wide_penumbra_and_a_point_a_sharp_edge() {
    let hard = penumbra_luxels(0.0, 1);
    let soft = penumbra_luxels(4.0, 16);
    assert!(hard <= 1, "point light edge is {hard} luxels");
    assert!(soft >= hard + 2, "soft {soft} vs hard {hard}");
}
