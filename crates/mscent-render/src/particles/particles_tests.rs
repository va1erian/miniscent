use super::*;
use crate::camera::Camera;
use crate::tiles::TileRenderer;
use mscent_math::Vec3;
use mscent_math::rng::Rng;
use std::time::Instant;

const W: u32 = 97;
const H: u32 = 70;

fn pt(x: f32, y: f32, inv_z: f32) -> ScreenPoint {
    ScreenPoint { x, y, inv_z }
}

fn sprite(x: f32, y: f32, radius: f32, inv_z: f32, mask: Mask) -> SpriteDraw {
    SpriteDraw {
        x,
        y,
        radius,
        inv_z,
        tint: 0xc080_a0ff,
        mask,
    }
}

/// A scene with something to test against everywhere: varied colour and a
/// depth field of stripes, a pure function of the screen position so a band
/// and a whole frame start out the same.
fn prepare(fb: &mut Framebuffer) {
    let (y0, y1) = fb.row_range();
    for y in y0..y1 {
        let (color, depth) = fb.row_mut(y).unwrap();
        for (x, (c, d)) in color.iter_mut().zip(depth.iter_mut()).enumerate() {
            let h = (x as u32).wrapping_mul(2654435761) ^ (y as u32).wrapping_mul(40503);
            *c = 0xff00_0000 | (h >> 3 & 0x00ff_ffff);
            *d = [0.0, 0.2, 0.45, 0.9][(x / 7 + y as usize / 5) % 4];
        }
    }
}

fn random_draws(seed: u64, count: u32) -> ParticleDraws {
    let mut rng = Rng::new(seed);
    let mut d = ParticleDraws::default();
    for i in 0..count {
        let masks = [
            Mask::Soft,
            Mask::Ring,
            Mask::Puff(rng.below(9) as u8),
            Mask::Fire(rng.below(20) as u8),
        ];
        let s = sprite(
            rng.range_f32(-20.0, W as f32 + 20.0),
            rng.range_f32(-20.0, H as f32 + 20.0),
            rng.range_f32(0.5, 40.0),
            rng.range_f32(0.1, 0.7),
            masks[rng.below(4) as usize],
        );
        let s = SpriteDraw {
            tint: rng.next_u64() as u32,
            ..s
        };
        if i % 2 == 0 {
            d.over.push(s)
        } else {
            d.add.push(s)
        }
    }
    for _ in 0..count {
        let mut p = || {
            pt(
                rng.range_f32(-30.0, W as f32 + 30.0),
                rng.range_f32(-30.0, H as f32 + 30.0),
                rng.range_f32(0.1, 0.7),
            )
        };
        let (a, b) = (p(), p());
        d.streaks.push(StreakDraw {
            a,
            b,
            width: rng.below(7) as u8,
            tint: rng.next_u64() as u32,
        });
    }
    d
}

fn reference(draws: &ParticleDraws, bank: &SpriteBank, level: Level) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).unwrap();
    prepare(&mut fb);
    draw_particles_with(level, &mut fb, draws, bank);
    fb
}

#[test]
fn draws_change_pixels_and_never_depth() {
    let bank = SpriteBank::new();
    let draws = random_draws(1, 40);
    let before = {
        let mut fb = Framebuffer::new(W, H).unwrap();
        prepare(&mut fb);
        fb
    };
    let after = reference(&draws, &bank, Level::best());
    assert_ne!(before.pixels(), after.pixels());
    let depth = |fb: &Framebuffer| -> Vec<Option<f32>> {
        (0..H as i32)
            .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
            .map(|(x, y)| fb.depth_at(x, y))
            .collect()
    };
    assert_eq!(depth(&before), depth(&after));
}

#[test]
fn every_level_draws_the_same_picture() {
    let bank = SpriteBank::new();
    for seed in 1..=6 {
        let draws = random_draws(seed, 60);
        let want = reference(&draws, &bank, Level::Scalar);
        for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
            assert_eq!(
                reference(&draws, &bank, level).pixels(),
                want.pixels(),
                "{level:?} seed {seed}"
            );
        }
    }
}

/// Left half is a wall at `1/z = wall`; the right half has nothing drawn.
fn walled(wall: f32) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).unwrap();
    let w = W as usize;
    for (i, d) in fb.planes_mut().1.iter_mut().enumerate() {
        *d = if i % w < w / 2 { wall } else { 0.0 };
    }
    fb
}

fn draws_for_depth_test(inv_z: f32) -> ParticleDraws {
    let mut d = ParticleDraws::default();
    let centre = (W as f32 / 2.0, H as f32 / 2.0);
    d.over
        .push(sprite(centre.0, centre.1, 30.0, inv_z, Mask::Soft));
    d.add
        .push(sprite(centre.0, centre.1, 30.0, inv_z, Mask::Soft));
    d.streaks.push(StreakDraw {
        a: pt(2.5, 10.5, inv_z),
        b: pt(W as f32 - 2.5, 10.5, inv_z),
        width: 3,
        tint: 0xffff_ffff,
    });
    d
}

#[test]
fn things_behind_a_wall_leave_its_pixels_alone() {
    let bank = SpriteBank::new();
    let w = W as i32;
    // Wall at 1/z 0.8 is nearer than the particles at 0.3.
    let mut fb = walled(0.8);
    let before = fb.clone();
    draw_particles(&mut fb, &draws_for_depth_test(0.3), &bank);
    let left: Vec<_> = (0..H as i32)
        .flat_map(|y| (0..w / 2).map(move |x| (x, y)))
        .collect();
    assert!(
        left.iter()
            .all(|&(x, y)| fb.pixel(x, y) == before.pixel(x, y))
    );
    let right_changed = (0..H as i32)
        .flat_map(|y| (w / 2..w).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.pixel(x, y) != before.pixel(x, y))
        .count();
    assert!(right_changed > 100, "the open half is drawn on");
}

#[test]
fn things_in_front_of_a_wall_change_its_pixels() {
    let bank = SpriteBank::new();
    // Wall at 0.3 is farther than the particles at 0.8.
    let mut fb = walled(0.3);
    let before = fb.clone();
    draw_particles(&mut fb, &draws_for_depth_test(0.8), &bank);
    let on_wall = |draws: &ParticleDraws, fb: &mut Framebuffer, blend_only: &str| {
        let mut one = ParticleDraws::default();
        match blend_only {
            "over" => one.over = draws.over.clone(),
            "add" => one.add = draws.add.clone(),
            _ => one.streaks = draws.streaks.clone(),
        }
        let b = fb.clone();
        draw_particles(fb, &one, &bank);
        (0..H as i32)
            .flat_map(|y| (0..W as i32 / 2).map(move |x| (x, y)))
            .filter(|&(x, y)| fb.pixel(x, y) != b.pixel(x, y))
            .count()
    };
    let draws = draws_for_depth_test(0.8);
    for kind in ["over", "add", "streak"] {
        let mut f = before.clone();
        assert!(on_wall(&draws, &mut f, kind) > 20, "{kind} shows in front");
    }
    assert_ne!(fb.pixels(), before.pixels());
}

#[test]
fn a_sprite_at_exactly_the_wall_depth_is_drawn() {
    let bank = SpriteBank::new();
    let mut fb = walled(0.5);
    let before = fb.clone();
    draw_particles(&mut fb, &draws_for_depth_test(0.5), &bank);
    assert_ne!(fb.pixels(), before.pixels());
    let mut fb2 = before.clone();
    draw_particles(&mut fb2, &draws_for_depth_test(0.4999), &bank);
    let left_changed =
        (0..H as i32).any(|y| (0..W as i32 / 2).any(|x| fb2.pixel(x, y) != before.pixel(x, y)));
    assert!(!left_changed);
}

#[test]
fn bands_of_any_height_and_worker_count_match_the_whole_frame() {
    let bank = SpriteBank::new();
    let draws = random_draws(77, 120);
    let want = reference(&draws, &bank, Level::best());
    assert_ne!(want.pixels(), {
        let mut fb = Framebuffer::new(W, H).unwrap();
        prepare(&mut fb);
        fb.pixels().to_vec()
    });
    for tile_rows in [1, 3, 8, 24, 69, 70, 200] {
        for workers in [1, 2, 5, 16] {
            let mut r = TileRenderer::new(W, H, tile_rows, workers).unwrap();
            let mut got = Framebuffer::new(W, H).unwrap();
            r.render(&mut got, &|band| {
                prepare(band);
                draw_particles(band, &draws, &bank);
            })
            .unwrap();
            assert_eq!(
                got.pixels(),
                want.pixels(),
                "tile_rows {tile_rows} workers {workers}"
            );
        }
    }
}

const HOSTILE: [f32; 14] = [
    f32::NAN,
    f32::INFINITY,
    f32::NEG_INFINITY,
    f32::MAX,
    f32::MIN,
    1.0e30,
    -1.0e30,
    f32::MIN_POSITIVE,
    -0.0,
    0.0,
    -1.0,
    1.0,
    1.0e9,
    40.0,
];

#[test]
fn hostile_values_never_panic_and_finish_quickly() {
    let bank = SpriteBank::new();
    let mut fb = Framebuffer::new(W, H).unwrap();
    prepare(&mut fb);
    let mut draws = ParticleDraws::default();
    for &x in &HOSTILE {
        for &y in &HOSTILE {
            for &radius in &HOSTILE {
                for &inv_z in &HOSTILE {
                    let s = sprite(x, y, radius, inv_z, Mask::Fire(255));
                    draws.over.push(s);
                    draws.add.push(SpriteDraw {
                        mask: Mask::Puff(201),
                        ..s
                    });
                }
            }
        }
    }
    let mut rng = Rng::new(5);
    let mut pick = || HOSTILE[rng.below(HOSTILE.len() as u32) as usize];
    for i in 0..30_000u32 {
        draws.streaks.push(StreakDraw {
            a: pt(pick(), pick(), pick()),
            b: pt(pick(), pick(), pick()),
            width: (i % 256) as u8,
            tint: 0xffff_ffff,
        });
    }
    // The exact case that would hang a pixel-by-pixel walk.
    for (a, b) in [
        (pt(-1.0e30, -1.0e30, 0.5), pt(1.0e30, 1.0e30, 0.5)),
        (pt(-1.0e30, 40.0, 0.5), pt(1.0e30, 41.0, 0.5)),
        (pt(f32::MIN, f32::MAX, 0.5), pt(f32::MAX, f32::MIN, 0.5)),
    ] {
        draws.streaks.push(StreakDraw {
            a,
            b,
            width: 5,
            tint: 0xffff_ffff,
        });
    }
    let start = Instant::now();
    for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
        draw_particles_with(level, &mut fb, &draws, &bank);
    }
    // Also through bands.
    let mut r = TileRenderer::new(W, H, 9, 3).unwrap();
    r.render(&mut fb, &|band| draw_particles(band, &draws, &bank))
        .unwrap();
    assert!(start.elapsed().as_secs() < 20, "took {:?}", start.elapsed());
}

#[test]
fn a_streak_at_plus_and_minus_1e30_returns_at_once() {
    let bank = SpriteBank::new();
    let mut fb = Framebuffer::new(1920, 1080).unwrap();
    let mut draws = ParticleDraws::default();
    draws.streaks.push(StreakDraw {
        a: pt(-1.0e30, -1.0e30, 0.5),
        b: pt(1.0e30, 1.0e30, 0.5),
        width: 5,
        tint: 0xffff_ffff,
    });
    let start = Instant::now();
    draw_particles(&mut fb, &draws, &bank);
    assert!(
        start.elapsed().as_millis() < 250,
        "took {:?}",
        start.elapsed()
    );
}

#[test]
fn empty_draw_list_changes_nothing_and_clear_keeps_capacity() {
    let bank = SpriteBank::new();
    let mut d = random_draws(3, 10);
    assert!(!d.is_empty());
    let cap = d.over.capacity();
    d.clear();
    assert!(d.is_empty());
    assert_eq!(d.over.capacity(), cap);
    let mut fb = Framebuffer::new(W, H).unwrap();
    prepare(&mut fb);
    let before = fb.clone();
    draw_particles(&mut fb, &d, &bank);
    assert_eq!(fb.pixels(), before.pixels());
}

#[test]
fn world_streak_draws_where_the_camera_sees_it() {
    let bank = SpriteBank::new();
    let cam = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        W,
        H,
    )
    .unwrap();
    let s = streak_from_world(
        &cam,
        Vec3::new(-2.0, 10.0, 0.0),
        Vec3::new(2.0, 10.0, 0.0),
        1,
        0xffff_ffff,
    )
    .unwrap();
    let mut draws = ParticleDraws::default();
    draws.streaks.push(s);
    let mut fb = Framebuffer::new(W, H).unwrap();
    draw_particles(&mut fb, &draws, &bank);
    // The segment crosses the screen centre, on the middle row.
    assert_ne!(fb.pixel(W as i32 / 2, H as i32 / 2 - 1), Some(0xff00_0000));
    assert_eq!(fb.pixel(W as i32 / 2, 5), Some(0xff00_0000));
}
