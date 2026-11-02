//! The half-res particle layer on real pixels: a real baked level, the real
//! depth buffer and the real band renderer at several tilings.

use miniscent_game::assets;
use mscent_game::fx::combat::{CombatFx, DRONE_TINT};
use mscent_math::{Vec3, Vec3d};
use mscent_render::particles::{Mask, ParticleDraws, SpriteBank, SpriteDraw};
use mscent_render::{BuildConfig, Framebuffer, LevelRenderer, Quality, View};
use mscent_world::{Brush, Entity, Level, Op};

// Odd on both sides, so the layer's last row and column are partial.
const W: u32 = 321;
const H: u32 = 181;
const FRAME: u64 = 16_666_667;
/// Pillar columns: its edge falls at 12 px from the middle at eye distance 10.
const MIDDLE: i32 = 160;

fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

/// A lit room with a pillar 2 wide, from floor to ceiling, 10 ahead of the eye.
fn renderer() -> LevelRenderer {
    let d = |x, y, z| Vec3d::new(x, y, z);
    let cuboid = |a, b, op, order| Brush::cuboid(a, b, op, order).expect("a brush");
    let level = Level {
        brushes: vec![
            cuboid(d(-30.0, -30.0, -20.0), d(30.0, 30.0, 20.0), Op::Subtract, 0),
            cuboid(d(-1.0, 0.0, -21.0), d(1.0, 2.0, 21.0), Op::Add, 1),
        ],
        entities: vec![Entity::Light {
            pos: v(0.0, -5.0, 5.0),
            color: v(1.0, 1.0, 1.0),
            radius: 60.0,
            intensity: 1.0,
        }],
        labels: Vec::new(),
        mover_groups: Vec::new(),
    };
    let cfg = BuildConfig {
        luxel_size: 1.0,
        shadow_samples: 1,
        ..BuildConfig::default()
    };
    let materials = assets::load_materials(None).expect("textures");
    LevelRenderer::from_level(&level, materials, &cfg).expect("a level")
}

fn view() -> View {
    View::from_angles(v(0.0, -10.0, 0.0), 0.0, 0.0, 0.0)
}

fn render(r: &LevelRenderer, draws: &ParticleDraws) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let bank = SpriteBank::new();
    r.render_with_particles(
        &mut fb,
        &view(),
        Quality::Low,
        &[],
        None,
        Some((draws, &bank)),
    )
    .expect("a frame");
    fb
}

fn big(inv_z: f32, tint: u32, mask: Mask) -> SpriteDraw {
    SpriteDraw {
        x: MIDDLE as f32 + 0.5,
        y: H as f32 / 2.0,
        radius: 100.0,
        inv_z,
        tint,
        mask,
    }
}

fn layered(inv_z: f32) -> ParticleDraws {
    let mut d = ParticleDraws::default();
    d.layer_over.push(big(inv_z, 0xb050_6080, Mask::Puff(2)));
    d.layer_add.push(big(inv_z, 0x6030_2010, Mask::Soft));
    d
}

/// The same sprites at full size.
fn flat(d: &ParticleDraws) -> ParticleDraws {
    ParticleDraws {
        over: d.layer_over.clone(),
        add: d.layer_add.clone(),
        ..ParticleDraws::default()
    }
}

fn differs(a: &Framebuffer, b: &Framebuffer, x: i32, y: i32) -> bool {
    a.pixel(x, y) != b.pixel(x, y)
}

/// Where the pillar is, from its depth: nearer than the sprite behind it.
fn on_pillar(base: &Framebuffer, x: i32, y: i32) -> bool {
    base.depth_at(x, y).is_some_and(|z| z > 0.07)
}

#[test]
fn a_pillar_in_front_of_a_layered_sprite_keeps_every_pixel() {
    let r = renderer();
    let base = render(&r, &ParticleDraws::default());
    // The sprite is 20 away; the pillar's face is 10 away.
    let with = render(&r, &layered(0.05));
    let (mut pillar, mut beside) = (0, 0);
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            if on_pillar(&base, x, y) {
                pillar += 1;
                assert!(
                    !differs(&base, &with, x, y),
                    "pillar pixel ({x}, {y}) changed"
                );
            } else if differs(&base, &with, x, y) {
                beside += 1;
            }
        }
    }
    assert!(pillar > 1000, "the pillar is in view: {pillar}");
    assert!(beside > 10_000, "and the smoke shows around it: {beside}");
}

#[test]
fn the_halo_at_a_pillar_edge_is_at_most_one_pixel_and_the_rest_agrees() {
    let r = renderer();
    let base = render(&r, &ParticleDraws::default());
    let d = layered(0.05);
    let (lay, full) = (render(&r, &d), render(&r, &flat(&d)));
    // Distance from the pillar's nearest column, for each row's open pixels.
    let (mut lost_near, mut lost_far) = (0, 0);
    let (mut sum, mut worst, mut n) = (0u64, 0u32, 0u64);
    for y in 10..H as i32 - 10 {
        let edge: Vec<i32> = (0..W as i32).filter(|&x| on_pillar(&base, x, y)).collect();
        let (Some(&first), Some(&last)) = (edge.first(), edge.last()) else {
            continue;
        };
        for x in 40..W as i32 - 40 {
            if on_pillar(&base, x, y) {
                continue;
            }
            let dist = (first - x).max(x - last);
            // Inside the sprite's solid part: its faint rim rounds to nothing in
            // the layer and not at full size, which is not a halo.
            let solid = (x - MIDDLE).pow(2) + (y - H as i32 / 2).pow(2) < 70 * 70;
            let missing =
                solid && lay.pixel(x, y) == base.pixel(x, y) && differs(&base, &full, x, y);
            if missing && dist <= 1 {
                lost_near += 1;
            } else if missing && dist > 1 {
                lost_far += 1;
            }
            if dist > 2 {
                let (a, b) = (lay.pixel(x, y).unwrap(), full.pixel(x, y).unwrap());
                let diff = (0..3)
                    .map(|i| ((a >> (8 * i)) & 255).abs_diff((b >> (8 * i)) & 255))
                    .max()
                    .unwrap_or(0);
                sum += u64::from(diff);
                worst = worst.max(diff);
                n += 1;
            }
        }
    }
    let mean = sum as f32 / n as f32;
    assert_eq!(lost_far, 0, "nothing is lost but beside the pillar");
    assert!(
        lost_near <= 2 * H as usize,
        "one pixel per side per row at most: {lost_near}"
    );
    assert!(mean < 1.5 && worst < 40, "mean {mean} worst {worst}");
}

#[test]
fn a_layered_sprite_in_front_of_the_pillar_shows_on_it() {
    let r = renderer();
    let base = render(&r, &ParticleDraws::default());
    let with = render(&r, &layered(0.9));
    let on_pillar_changed = (0..H as i32)
        .flat_map(|y| (MIDDLE - 8..MIDDLE + 8).map(move |x| (x, y)))
        .filter(|&(x, y)| differs(&base, &with, x, y))
        .count();
    assert!(on_pillar_changed > 500, "{on_pillar_changed}");
}

#[test]
fn every_worker_count_gives_identical_pixels_at_even_tilings() {
    let mut r = renderer();
    let d = layered(0.05);
    let mut shots = Vec::new();
    // 24 rows is the default; 180 holds the frame in one band bar a row.
    for (rows, workers) in [(24, 1), (24, 4), (24, 24), (48, 3), (2, 7), (181, 2)] {
        r.set_tiling(rows, workers);
        shots.push((rows, workers, render(&r, &d).pixels().to_vec()));
    }
    let (rows, workers, want) = &shots[0];
    for (r, w, got) in &shots {
        assert!(
            got == want,
            "{r} rows x {w} workers against {rows} x {workers}"
        );
    }
}

#[test]
fn bands_that_cannot_hold_the_layer_draw_it_full_size_for_any_worker_count() {
    let mut r = renderer();
    let d = layered(0.05);
    r.set_tiling(7, 1);
    let one = render(&r, &d).pixels().to_vec();
    for workers in [2, 5, 16] {
        r.set_tiling(7, workers);
        assert!(render(&r, &d).pixels() == one, "{workers} workers");
    }
    // Close to the layered picture (both draw the same sprites), though not
    // the same: the bands that fit use the layer.
    r.set_tiling(24, 1);
    let layered = render(&r, &d);
    let diff = layered
        .pixels()
        .iter()
        .zip(&one)
        .filter(|(a, b)| a != b)
        .count();
    assert!(diff > 0, "the two paths are not the same picture");
}

#[test]
fn a_crowd_of_explosions_draws_the_same_at_any_worker_count() {
    let mut r = renderer();
    // A crowd that overruns the budget, so the layer is used.
    let mut fx = CombatFx::new();
    for i in 0..120 {
        let spread = i as f32 / 120.0 - 0.5;
        fx.drone_explosion(
            v(spread * 16.0, -4.0 + spread * 4.0, spread * 6.0),
            DRONE_TINT,
        );
    }
    for _ in 0..12 {
        fx.advance(FRAME, r.scene().bsp());
    }
    let cam = r.camera(&view(), W, H).expect("a camera");
    let mut draws = CombatFx::new_draws();
    fx.collect(&cam, r.fog(), None, &mut draws);
    let n = draws.layer_over.len() + draws.layer_add.len();
    assert!(n > 20, "the layer is in use: {n}");
    let mut shots = Vec::new();
    for (rows, workers) in [(24, 1), (24, 4), (24, 24), (48, 2)] {
        r.set_tiling(rows, workers);
        shots.push(render(&r, &draws).pixels().to_vec());
    }
    assert!(shots.windows(2).all(|w| w[0] == w[1]), "bands disagree");
}
