//! Review check: a cache file with a valid magic, key and checksum but forged
//! contents (the checksum is not a MAC) must never crash the renderer. Unlike
//! plain byte damage, resealed damage gets past the checksum and exercises the
//! field validation and every consumer of the lightmap.

use std::path::Path;

use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_render::{
    BakeCache, BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::showcase_level;

fn materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

fn reseal(b: &mut [u8]) {
    let n = b.len() - 8;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &x in &b[..n] {
        h = (h ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    b[n..].copy_from_slice(&h.to_le_bytes());
}

#[test]
fn resealed_forgeries_never_crash_the_renderer() {
    let level = showcase_level();
    let path = std::env::temp_dir().join(format!("mscent_forged_{}.lit", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let cfg = BuildConfig::default();
    LevelRenderer::from_level_cached(
        &level,
        materials(),
        &cfg,
        &mut BakeCache::with_file(path.clone()),
    )
    .expect("first build");
    let good = std::fs::read(&path).expect("cache file");
    let face_table = 26..(good.len().min(26 + 4000));
    let mut rng = Rng::new(11);
    let mut accepted = 0;
    for round in 0..24 {
        let mut b = good.clone();
        for _ in 0..3 {
            // Mostly the face table (rects and projections), sometimes luxels.
            let i = if round % 4 == 0 {
                14 + (rng.next_u64() as usize) % (b.len() - 22)
            } else {
                face_table.start + (rng.next_u64() as usize) % face_table.len()
            };
            b[i] = match rng.next_u64() % 4 {
                0 => 0x7f,
                1 => 0xff,
                2 => 0x00,
                _ => rng.next_u64() as u8,
            };
        }
        reseal(&mut b);
        std::fs::write(&path, &b).expect("write");
        let mut cache = BakeCache::with_file(path.clone());
        let r = LevelRenderer::from_level_cached(&level, materials(), &cfg, &mut cache)
            .expect("build never fails because of the cache");
        accepted += u32::from(cache.bakes() == 0);
        let mut fb = Framebuffer::new(80, 60).expect("fb");
        let view = View::from_angles(Vec3::new(0.0, 0.0, 2.0), 0.4, 0.0, 0.0);
        for q in [Quality::Low, Quality::Medium, Quality::High] {
            let _ = r.render(&mut fb, &view, q, &[], None);
        }
    }
    let _ = std::fs::remove_file(&path);
    assert!(accepted > 0, "some forgeries should have passed validation");
}
