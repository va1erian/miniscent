//! The baked light of voxel surfaces is kept with the lightmap: painting a
//! volume never bakes again, carving one does, and a stale or forged file is
//! ignored and never reaches the renderer.

#[allow(dead_code)]
#[path = "support/crawl.rs"]
mod crawl;
#[allow(dead_code)]
#[path = "support/stability.rs"]
mod stability;
#[allow(dead_code)]
#[path = "support/voxel_scene.rs"]
mod voxel_scene;

use std::path::PathBuf;

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BakeCache, BuildConfig, Compiled, Framebuffer, LevelRenderer, Quality, Source, View,
};
use mscent_world::Level;
use mscent_world::materials::BRICK;
use mscent_world::voxel::{Context, DabOp, Prim, apply_dab};
use stability::Look;

fn config() -> BuildConfig {
    BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 4,
        ..stability::config()
    }
}

fn level() -> Level {
    voxel_scene::shadowed_room()
}

/// The room with a ball of the volume's floor changed by `op`.
fn edited(op: DabOp) -> Level {
    let mut level = level();
    let shape = level.volumes[0].clone();
    let brushes = level.brushes.clone();
    let ball = Prim::sphere(Vec3d::new(16.0, 5.0, 0.0), 1.0).expect("sphere");
    apply_dab(
        &mut level.volumes[0],
        &mut Context::new(&shape, &brushes),
        &ball,
        op,
    );
    level
}

fn build(level: &Level, cache: &mut BakeCache) -> LevelRenderer {
    let materials = stability::materials_for(Look::Flat);
    LevelRenderer::from_level_cached(level, materials, &config(), cache).expect("builds")
}

fn key(level: &Level) -> u64 {
    Compiled::new(level, &config())
        .expect("compiles")
        .light_key()
}

fn temp_file(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("mscent_voxel_light_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir.join("level.msl.lit")
}

fn reseal(b: &mut [u8]) {
    let n = b.len() - 8;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in &b[..n] {
        h = (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    b[n..].copy_from_slice(&h.to_le_bytes());
}

fn note(cache: &BakeCache) -> String {
    cache
        .last_report()
        .and_then(|r| r.note.clone())
        .unwrap_or_default()
}

/// Draws a frame from where the room's voxel floor is in view.
fn draw(r: &LevelRenderer) {
    let mut fb = Framebuffer::new(80, 60).expect("framebuffer");
    let view = View::from_angles(Vec3::new(18.0, 12.0, 4.0), 2.6, -0.6, 0.0);
    for q in [Quality::Low, Quality::Medium, Quality::High] {
        r.render(&mut fb, &view, q, &[], None).expect("renders");
    }
}

#[test]
fn painting_a_volume_keeps_the_light_key_and_carving_it_changes_it() {
    let base = key(&level());
    let painted = edited(DabOp::Paint {
        material: BRICK as u8,
    });
    assert_ne!(
        painted.volumes,
        level().volumes,
        "the dab changed the volume"
    );
    assert_eq!(base, key(&painted));
    assert_ne!(base, key(&edited(DabOp::Carve)));
}

#[test]
fn painting_does_not_bake_again_and_carving_does() {
    let mut cache = BakeCache::new();
    build(&level(), &mut cache);
    assert_eq!(cache.bakes(), 1);
    build(
        &edited(DabOp::Paint {
            material: BRICK as u8,
        }),
        &mut cache,
    );
    assert_eq!(
        (cache.bakes(), cache.last_source()),
        (1, Some(Source::Memory)),
        "painting found the light it had"
    );
    build(&edited(DabOp::Carve), &mut cache);
    assert_eq!(
        cache.bakes(),
        2,
        "carving changed the surface light falls on"
    );
}

#[test]
fn a_cache_file_of_another_shape_is_stale_and_the_level_bakes_again() {
    let path = temp_file("stale");
    build(&level(), &mut BakeCache::with_file(path.clone()));
    let mut cache = BakeCache::with_file(path.clone());
    let r = build(&edited(DabOp::Carve), &mut cache);
    assert_eq!(cache.bakes(), 1);
    assert!(note(&cache).contains("stale"), "{}", note(&cache));
    draw(&r);
    // The new bake replaced the file, so the carved level now loads from it.
    let mut again = BakeCache::with_file(path.clone());
    build(&edited(DabOp::Carve), &mut again);
    assert_eq!(
        (again.bakes(), again.last_source()),
        (0, Some(Source::File))
    );
    let _ = std::fs::remove_file(&path);
}

/// The bytes of a good cache file and how many voxel records it ends with.
fn good_file(path: &PathBuf) -> (Vec<u8>, usize) {
    let r = build(&level(), &mut BakeCache::with_file(path.clone()));
    let cells = r.lightmap().cells().len();
    assert!(cells > 100, "the level has voxel surface: {cells} cells");
    (std::fs::read(path).expect("cache file"), cells)
}

#[test]
fn forged_voxel_counts_are_ignored_and_the_level_bakes_again() {
    let path = temp_file("counts");
    let (good, cells) = good_file(&path);
    let count_at = good.len() - 8 - cells * 9 - 4;
    // A count that does not fit the bytes, and one that fits but is not this
    // level's: no records at all, sealed with a valid checksum.
    let mut lies = Vec::new();
    for lie in [cells as u32 + 1, u32::MAX] {
        let mut b = good.clone();
        b[count_at..count_at + 4].copy_from_slice(&lie.to_le_bytes());
        lies.push((b, "damaged"));
    }
    let mut empty = good[..count_at].to_vec();
    empty.extend_from_slice(&0u32.to_le_bytes());
    empty.extend_from_slice(&[0; 8]);
    lies.push((empty, "voxel cell count differs"));
    for (mut b, expected) in lies {
        reseal(&mut b);
        std::fs::write(&path, &b).expect("write");
        let mut cache = BakeCache::with_file(path.clone());
        let r = build(&level(), &mut cache);
        assert_eq!(cache.bakes(), 1, "{expected}");
        assert!(
            note(&cache).contains(expected),
            "{expected}: {}",
            note(&cache)
        );
        assert_eq!(r.lightmap().cells().len(), cells);
        draw(&r);
    }
    let _ = std::fs::remove_file(&path);
}

#[test]
fn any_bytes_in_the_records_are_light_and_never_crash_the_renderer() {
    let path = temp_file("records");
    let (good, cells) = good_file(&path);
    let first = good.len() - 8 - cells * 9;
    let mut rng = Rng::new(5);
    for _ in 0..8 {
        let mut b = good.clone();
        for _ in 0..200 {
            let at = first + (rng.next_u64() as usize) % (cells * 9);
            b[at] = rng.next_u64() as u8;
        }
        reseal(&mut b);
        std::fs::write(&path, &b).expect("write");
        let mut cache = BakeCache::with_file(path.clone());
        let r = build(&level(), &mut cache);
        assert_eq!(cache.bakes(), 0, "records are data: {}", note(&cache));
        draw(&r);
    }
    let _ = std::fs::remove_file(&path);
}
