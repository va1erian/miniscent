//! Review checks on what a cached lightmap relies on: the baker gives the
//! same bytes for any worker count, and any change of the baker's output must
//! come with a `store::VERSION` bump (else a stale `.msl.lit` from the old
//! baker keeps being served and the user sees the old shadows).

use mscent_math::Vec3;
use mscent_world::lightmap::store::{VERSION, input_key};
use mscent_world::lightmap::{BakeConfig, LightFace, PointLight, bake};
use mscent_world::{Entity, compile, extract_faces, showcase_level};

fn scene() -> (mscent_world::Bsp, Vec<LightFace>, Vec<PointLight>) {
    let level = showcase_level();
    let bsp = compile(&level.brushes).expect("compiles");
    let faces: Vec<LightFace> = extract_faces(&bsp, &level.brushes)
        .iter()
        .map(LightFace::from)
        .collect();
    let lights = level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => Some(PointLight {
                position: pos,
                color: color * intensity,
                radius,
            }),
            _ => None,
        })
        .collect();
    (bsp, faces, lights)
}

fn cfg(workers: usize) -> BakeConfig {
    BakeConfig {
        luxel_size: 0.5,
        workers,
        light_radius: 0.3,
        shadow_samples: 16,
        bounce_gain: 0.18,
        ambient: Vec3::new(0.03, 0.03, 0.03),
        ..BakeConfig::default()
    }
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| {
        (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn the_baked_bytes_do_not_depend_on_the_worker_count() {
    let (bsp, faces, lights) = scene();
    let key = input_key(&faces, &lights, &cfg(1));
    let reference = bake(&bsp, &faces, &lights, &cfg(1))
        .expect("bakes")
        .to_bytes(key);
    for workers in [2, 3, 7, 64] {
        assert_eq!(input_key(&faces, &lights, &cfg(workers)), key);
        let bytes = bake(&bsp, &faces, &lights, &cfg(workers))
            .expect("bakes")
            .to_bytes(key);
        assert!(bytes == reference, "{workers} workers changed the bake");
    }
}

/// Pins the baker's output for a fixed scene to the file format version. When
/// this fails the baker changed what it writes: bump `store::VERSION`, then
/// update the pair below with the printed fingerprint. Without the bump, cache
/// files written by the old baker would still match their key and be reused.
/// The fingerprint hashes luxels rounded to 1/32, so last-bit differences
/// between machines do not trip it.
#[test]
fn a_change_of_baker_output_needs_a_version_bump() {
    const PINNED: (u16, u64) = (4, 0xebc4_7bbf_19bb_6735);
    let (bsp, faces, lights) = scene();
    let map = bake(&bsp, &faces, &lights, &cfg(1)).expect("bakes");
    let mut bytes = Vec::new();
    for (i, p) in map.projections().iter().enumerate() {
        for y in 0..p.height {
            for x in 0..p.width {
                let l = map.face_luxel(i, x, y).expect("inside the face");
                for v in [l.color.x, l.color.y, l.color.z, l.dir.x, l.dir.y, l.dir.z] {
                    bytes.extend_from_slice(&((v * 32.0).round() as i32).to_le_bytes());
                }
            }
        }
    }
    let got = fnv(&bytes);
    assert_eq!(
        (VERSION, got),
        PINNED,
        "baker output changed: bump store::VERSION and pin ({VERSION}, {got:#x})"
    );
}
