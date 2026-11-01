//! The pillar's shadow in the showcase start room must read clearly: the
//! baked floor behind the pillar is much darker than the lit floor at a
//! similar distance from the lamp.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{BuildConfig, LevelRenderer, MaterialTable};
use mscent_world::showcase_level;

fn luma(c: Vec3) -> f32 {
    0.3 * c.x + 0.59 * c.y + 0.11 * c.z
}

#[test]
fn pillar_shadow_is_clearly_darker_than_the_lit_floor() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    let r = LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .expect("level builds");
    let lm = r.lightmap();
    // The lamp is at (6, 6, 6.5); the pillar fills (11..13, 11..13).
    let spot = |x: f32, y: f32| Vec3::new(x, y, 0.0);
    // The floor face whose luxel grid covers a point (sampling clamps).
    let covers = |i: usize, p: Vec3| {
        let q = &lm.projections()[i];
        let d = p - q.origin;
        let (u, v) = (d.dot(q.tangent) - q.u_min, d.dot(q.bitangent) - q.v_min);
        let (w, h) = (
            q.width as f32 * q.luxel_size,
            q.height as f32 * q.luxel_size,
        );
        q.normal.z > 0.99
            && q.origin.z.abs() < 0.01
            && (0.0..w).contains(&u)
            && (0.0..h).contains(&v)
    };
    let face_at = |p: Vec3| {
        (0..lm.projections().len())
            .find(|&i| covers(i, p))
            .expect("floor face")
    };
    let at = |x: f32, y: f32| {
        let p = spot(x, y);
        luma(lm.sample_world(face_at(p), p).expect("luxel").color)
    };
    let (shadow, lit) = (at(15.5, 15.5), at(18.0, 6.0));
    assert!(lit > 6.0 * shadow, "shadow {shadow} vs lit {lit}");
}
