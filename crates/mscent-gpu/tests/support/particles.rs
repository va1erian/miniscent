//! Particle draws for the parity cases, placed in the world ahead of a view and
//! projected by the frame's camera, as the game's effects make them: some in
//! front of the scene and some behind it, so the depth test shows.

use mscent_math::Vec3;
use mscent_render::particles::{
    DecalVertex, Mask, ParticleDraws, SpriteDraw, decal_from_world, streak_from_world,
};
use mscent_render::{Camera, View};

/// Which particles a case draws.
#[derive(Clone, Copy)]
pub enum Sparks {
    /// Additive fireballs and rings.
    Added,
    /// Puffs drawn over each other, back to front.
    Over,
    /// Streaks of each width, some running into the walls.
    Streaks,
    /// A scorch mark on the surface at `centre`, facing `normal`.
    Decal { centre: Vec3, normal: Vec3 },
}

/// The view's right and up, for spreading draws across it.
fn axes(view: &View) -> (Vec3, Vec3, Vec3) {
    let forward = view.forward();
    let right = forward
        .cross(Vec3::new(0.0, 0.0, 1.0))
        .normalized()
        .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
    (forward, right, right.cross(forward))
}

/// A sprite `world` units across at `at`, or nothing behind the eye.
fn sprite(cam: &Camera, at: Vec3, world: f32, tint: u32, mask: Mask) -> Option<SpriteDraw> {
    let p = cam.project(cam.world_to_camera(at))?;
    Some(SpriteDraw {
        x: p.x,
        y: p.y,
        radius: world * cam.focal() * p.inv_z,
        inv_z: p.inv_z,
        tint,
        mask,
    })
}

/// Points ahead of the eye, `near` to `far` along the view and spread across it.
fn spread(view: &View, near: f32, far: f32, n: usize) -> Vec<Vec3> {
    let (forward, right, up) = axes(view);
    (0..n)
        .map(|i| {
            let t = i as f32 / (n - 1).max(1) as f32;
            let across = (t - 0.5) * 4.0;
            let lift = ((i * 7) % 5) as f32 * 0.35 - 0.7;
            view.pos + forward * (near + (far - near) * t) + right * across + up * lift
        })
        .collect()
}

pub fn draws(kind: Sparks, cam: &Camera, view: &View) -> ParticleDraws {
    let mut out = ParticleDraws::default();
    match kind {
        Sparks::Added => {
            for (i, at) in spread(view, 2.0, 30.0, 12).into_iter().enumerate() {
                let mask = if i % 3 == 0 {
                    Mask::Ring
                } else {
                    Mask::Fire(i as u8)
                };
                let tint = 0xc0ff_a040 - (i as u32) * 0x0004_0800;
                out.add
                    .extend(sprite(cam, at, 0.8 + i as f32 * 0.15, tint, mask));
            }
        }
        Sparks::Over => {
            // Back to front, as the caller sorts them.
            for (i, at) in spread(view, 2.5, 30.0, 12).into_iter().rev().enumerate() {
                let tint = 0xb050_4848 + (i as u32) * 0x0002_0202;
                out.over
                    .extend(sprite(cam, at, 1.2, tint, Mask::Puff(i as u8)));
            }
        }
        Sparks::Streaks => {
            let points = spread(view, 1.5, 40.0, 10);
            let (_, right, up) = axes(view);
            for (i, &a) in points.iter().enumerate() {
                let b = a + right * 3.0 + up * (i as f32 * 0.4 - 1.5) + view.forward() * 6.0;
                let width = (i % 5) as u8 + 1;
                out.streaks
                    .extend(streak_from_world(cam, a, b, width, 0xffff_d060));
            }
        }
        Sparks::Decal { centre, normal } => {
            let u = normal
                .cross(Vec3::new(0.0, 0.0, 1.0))
                .normalized()
                .unwrap_or(Vec3::new(1.0, 0.0, 0.0));
            let v = u.cross(normal);
            let on = centre + normal * 0.01;
            let corner = |du: f32, dv: f32, s, t| DecalVertex {
                pos: on + u * du + v * dv,
                s,
                t,
            };
            let verts = [
                corner(-1.5, -1.5, 0.0, 0.0),
                corner(-1.5, 1.5, 0.0, 1.0),
                corner(1.5, 1.5, 1.0, 1.0),
                corner(1.5, -1.5, 1.0, 0.0),
            ];
            let draw = decal_from_world(cam, &verts, 220, Mask::Puff(2));
            let draw = draw.or_else(|| {
                let mut back = verts;
                back.reverse();
                decal_from_world(cam, &back, 220, Mask::Puff(2))
            });
            out.decals.extend(draw);
        }
    }
    out
}
