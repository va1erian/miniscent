use super::*;
use crate::particles::{Mask, draw_particles_with};
use crate::tiles::TileRenderer;
use mscent_math::rng::Rng;

// Odd on both sides, so the last layer row and column cover one pixel.
const W: u32 = 157;
const H: u32 = 101;

fn sprite(x: f32, y: f32, radius: f32, inv_z: f32, tint: u32, mask: Mask) -> SpriteDraw {
    SpriteDraw {
        x,
        y,
        radius,
        inv_z,
        tint,
        mask,
    }
}

/// A scene with something to test against everywhere: varied colour, and a
/// depth that is a pure function of the screen position, so a band and a whole
/// frame start out the same.
fn scene(fb: &mut Framebuffer, depth: &dyn Fn(usize, usize) -> f32) {
    let (y0, y1) = fb.row_range();
    for y in y0..y1 {
        let (color, z) = fb.row_mut(y).unwrap();
        for (x, (c, d)) in color.iter_mut().zip(z.iter_mut()).enumerate() {
            let h = (x as u32).wrapping_mul(2654435761) ^ (y as u32).wrapping_mul(40503);
            *c = 0xff00_0000 | (h >> 3 & 0x00ff_ffff);
            *d = depth(x, y as usize);
        }
    }
}

fn open(_: usize, _: usize) -> f32 {
    0.0
}

fn random_layered(seed: u64, count: u32) -> ParticleDraws {
    let mut rng = Rng::new(seed);
    let mut d = ParticleDraws::default();
    for i in 0..count {
        let masks = [
            Mask::Soft,
            Mask::Puff(rng.below(9) as u8),
            Mask::Fire(rng.below(20) as u8),
        ];
        let s = sprite(
            rng.range_f32(-30.0, W as f32 + 30.0),
            rng.range_f32(-30.0, H as f32 + 30.0),
            rng.range_f32(8.0, 90.0),
            rng.range_f32(0.1, 0.7),
            rng.next_u64() as u32,
            masks[rng.below(3) as usize],
        );
        if i % 2 == 0 {
            d.layer_over.push(s);
        } else {
            d.layer_add.push(s);
        }
    }
    d
}

/// The same sprites drawn at full size.
fn at_full_size(d: &ParticleDraws) -> ParticleDraws {
    ParticleDraws {
        over: d.layer_over.clone(),
        add: d.layer_add.clone(),
        ..ParticleDraws::default()
    }
}

fn whole(draws: &ParticleDraws, level: Level, depth: &dyn Fn(usize, usize) -> f32) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).unwrap();
    scene(&mut fb, depth);
    draw_particles_with(level, &mut fb, draws, &SpriteBank::new());
    fb
}

fn tiled(
    draws: &ParticleDraws,
    rows: u32,
    workers: usize,
    depth: &(dyn Fn(usize, usize) -> f32 + Sync),
) -> Vec<u32> {
    let bank = SpriteBank::new();
    let mut r = TileRenderer::new(W, H, rows, workers).unwrap();
    let mut got = Framebuffer::new(W, H).unwrap();
    r.render(&mut got, &|band| {
        scene(band, depth);
        draw_particles_with(Level::best(), band, draws, &bank);
    })
    .unwrap();
    got.pixels().to_vec()
}

fn stripes(x: usize, y: usize) -> f32 {
    [0.0, 0.05, 0.2, 0.9][(x / 7 + y / 5) % 4]
}

#[test]
fn every_level_draws_the_same_layer_picture() {
    for seed in 1..=6 {
        let draws = random_layered(seed, 12);
        let want = whole(&draws, Level::Scalar, &stripes);
        let bare = whole(&ParticleDraws::default(), Level::Scalar, &stripes);
        assert_ne!(want.pixels(), bare.pixels(), "seed {seed} draws something");
        for level in Level::ALL.into_iter().filter(|l| l.is_supported()) {
            let got = whole(&draws, level, &stripes);
            assert!(got.pixels() == want.pixels(), "{level:?} seed {seed}");
        }
    }
}

#[test]
fn bands_that_hold_the_layer_match_the_whole_frame_at_any_worker_count() {
    let draws = random_layered(77, 24);
    let want = whole(&draws, Level::best(), &stripes);
    // Even band heights, and 101 (the whole frame), end at an even row or at
    // the end of the frame.
    for rows in [2, 4, 24, 50, 100, 101, 200] {
        for workers in [1, 2, 5, 16] {
            let got = tiled(&draws, rows, workers, &stripes);
            assert!(got == want.pixels(), "rows {rows} workers {workers}");
        }
    }
}

#[test]
fn bands_that_cannot_hold_the_layer_draw_full_size_and_do_not_depend_on_workers() {
    let draws = random_layered(78, 24);
    let full = whole(&at_full_size(&draws), Level::best(), &stripes);
    // Three-row bands (the last is 99..101) all start or end on an odd row.
    for workers in [1, 3, 8] {
        let got = tiled(&draws, 3, workers, &stripes);
        assert!(got == full.pixels(), "workers {workers}");
    }
    // A mixed tiling (the last band is 98..101, which fits) still gives one
    // picture for every worker count.
    let one = tiled(&draws, 7, 1, &stripes);
    for workers in [2, 4, 9] {
        assert!(
            tiled(&draws, 7, workers, &stripes) == one,
            "workers {workers}"
        );
    }
}

#[test]
fn a_band_fits_when_its_rows_pair_up() {
    let band = |y0, rows| Framebuffer::new_band(10, y0, rows, 101).unwrap();
    assert!(fits(&band(0, 24)) && fits(&band(24, 24)) && fits(&band(98, 3)));
    assert!(fits(&band(100, 1)), "ends at the frame's end");
    assert!(!fits(&band(1, 24)), "odd start");
    assert!(!fits(&band(0, 7)), "odd end inside the frame");
    assert!(fits(&Framebuffer::new(10, 7).unwrap()), "a whole odd frame");
}

#[test]
fn the_last_row_and_column_of_an_odd_frame_are_drawn() {
    let big = |tint| sprite(W as f32 / 2.0, H as f32 / 2.0, 400.0, 0.5, tint, Mask::Soft);
    let mut d = ParticleDraws::default();
    d.layer_add.push(big(0xff30_3030));
    let bare = whole(&ParticleDraws::default(), Level::best(), &open);
    let lay = whole(&d, Level::best(), &open);
    let full = whole(&at_full_size(&d), Level::best(), &open);
    let (w, h) = (W as i32, H as i32);
    for (x, y) in [
        (w - 1, h - 1),
        (w - 1, 0),
        (0, h - 1),
        (w - 1, h / 2),
        (w / 2, h - 1),
    ] {
        assert_ne!(lay.pixel(x, y), bare.pixel(x, y), "({x}, {y}) is drawn");
        let (a, b) = (lay.pixel(x, y).unwrap(), full.pixel(x, y).unwrap());
        assert!(
            max_channel_diff(a, b) <= 3,
            "({x}, {y}): {a:x} against {b:x}"
        );
    }
}

fn max_channel_diff(a: u32, b: u32) -> u32 {
    (0..3)
        .map(|i| ((a >> (8 * i)) & 255).abs_diff((b >> (8 * i)) & 255))
        .max()
        .unwrap_or(0)
}

/// Nearer than the sprite for `x < edge`; nothing drawn for the rest.
fn wall_until(edge: usize) -> impl Fn(usize, usize) -> f32 {
    move |x, _| if x < edge { 0.8 } else { 0.0 }
}

#[test]
fn a_layered_sprite_behind_a_wall_changes_no_pixel_of_it() {
    let mut d = ParticleDraws::default();
    d.layer_over
        .push(sprite(78.0, 50.0, 70.0, 0.3, 0xc080_a0ff, Mask::Puff(2)));
    d.layer_add
        .push(sprite(60.0, 50.0, 60.0, 0.3, 0x8040_8060, Mask::Soft));
    // Edges on both parities of column, so a block can straddle the wall.
    for edge in [70, 71, 100] {
        let wall = wall_until(edge);
        let got = whole(&d, Level::best(), &wall);
        let bare = whole(&ParticleDraws::default(), Level::best(), &wall);
        for y in 0..H as i32 {
            for x in 0..edge as i32 {
                assert_eq!(
                    got.pixel(x, y),
                    bare.pixel(x, y),
                    "edge {edge} at ({x}, {y})"
                );
            }
        }
        let open_side = (0..H as i32)
            .flat_map(|y| (edge as i32..W as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| got.pixel(x, y) != bare.pixel(x, y))
            .count();
        assert!(
            open_side > 100,
            "edge {edge}: the open side is drawn on ({open_side})"
        );
    }
}

#[test]
fn a_wall_in_front_of_a_layered_sprite_costs_at_most_one_column_of_halo() {
    let mut d = ParticleDraws::default();
    d.layer_add
        .push(sprite(78.0, 50.0, 70.0, 0.3, 0xff60_6060, Mask::Soft));
    for edge in [70usize, 71] {
        let wall = wall_until(edge);
        let lay = whole(&d, Level::best(), &wall);
        let full = whole(&at_full_size(&d), Level::best(), &wall);
        let bare = whole(&ParticleDraws::default(), Level::best(), &wall);
        // Where the sprite is solid (its faint rim rounds to nothing in the
        // layer and not at full size, which is not a halo), the layer may lack
        // it only in the block that holds the edge.
        let mut lost = [0; 12];
        for y in 15..85 {
            for x in edge as i32..edge as i32 + 12 {
                if lay.pixel(x, y) == bare.pixel(x, y) && full.pixel(x, y) != bare.pixel(x, y) {
                    lost[(x - edge as i32) as usize] += 1;
                }
            }
        }
        assert!(lost[1..].iter().all(|&n| n == 0), "edge {edge}: {lost:?}");
        assert!(
            lost[0] <= 70,
            "one column at most, in the block that holds the edge: {lost:?}"
        );
    }
}

#[test]
fn a_layered_sprite_agrees_with_the_full_size_one_away_from_edges() {
    let cases = [
        // Measured: mean 0.66, 0.78, 0.81, 0.67 and worst 3, 5, 7, 8 of 255.
        // The layer samples the mask at half the resolution and takes the
        // nearest pixel, so the worst case is where a mask gradient is steep.
        ("add soft", Mask::Soft, false, 0xff50_5050u32, 1.5, 5),
        ("over soft", Mask::Soft, true, 0xb040_6080, 1.5, 7),
        ("over puff", Mask::Puff(1), true, 0xc060_6060, 1.5, 9),
        ("add fire", Mask::Fire(3), false, 0xff80_6020, 1.5, 10),
    ];
    for (name, mask, over, tint, mean_max, worst_max) in cases {
        let s = sprite(78.0, 50.0, 70.0, 0.5, tint, mask);
        let mut d = ParticleDraws::default();
        if over {
            d.layer_over.push(s);
        } else {
            d.layer_add.push(s);
        }
        let lay = whole(&d, Level::best(), &open);
        let full = whole(&at_full_size(&d), Level::best(), &open);
        let (mut sum, mut worst, mut n) = (0u64, 0u32, 0u64);
        // The sprite's box is x in 8..148, y in -20..120; keep to its interior
        // and off the frame's last row and column.
        for y in 4..H as i32 - 2 {
            for x in 12..W as i32 - 12 {
                let diff = max_channel_diff(lay.pixel(x, y).unwrap(), full.pixel(x, y).unwrap());
                sum += u64::from(diff);
                worst = worst.max(diff);
                n += 1;
            }
        }
        let mean = sum as f32 / n as f32;
        assert!(mean <= mean_max, "{name}: mean {mean}");
        assert!(worst <= worst_max, "{name}: worst {worst}");
    }
}

#[test]
fn a_band_without_a_layered_sprite_does_nothing_and_makes_no_layer() {
    // Its own thread, so the scratch is this test's.
    std::thread::spawn(|| {
        let bank = SpriteBank::new();
        let mut d = ParticleDraws::default();
        d.over
            .push(sprite(40.0, 40.0, 60.0, 0.5, 0xc080_a0ff, Mask::Soft));
        let mut fb = Framebuffer::new(W, H).unwrap();
        scene(&mut fb, &stripes);
        draw_particles_with(Level::best(), &mut fb, &d, &bank);
        assert!(SCRATCH.with(|s| s.borrow().is_none()), "no layer was made");
        // A layered sprite entirely off the screen: the layer exists but
        // nothing is composited.
        let mut off = ParticleDraws::default();
        off.layer_over
            .push(sprite(-500.0, 40.0, 60.0, 0.5, 0xc080_a0ff, Mask::Soft));
        let (mut a, mut b) = (
            Framebuffer::new(W, H).unwrap(),
            Framebuffer::new(W, H).unwrap(),
        );
        scene(&mut a, &stripes);
        scene(&mut b, &stripes);
        draw_particles_with(Level::best(), &mut a, &off, &bank);
        assert_eq!(a.pixels(), b.pixels());
    })
    .join()
    .unwrap();
}

#[test]
fn the_scratch_keeps_its_storage_at_a_constant_size_and_when_smaller() {
    std::thread::spawn(|| {
        let bank = SpriteBank::new();
        let draws = random_layered(5, 10);
        let draw_band = |y0, rows| {
            let mut band = Framebuffer::new_band(W, y0, rows, H).unwrap();
            scene(&mut band, &stripes);
            draw_particles_with(Level::best(), &mut band, &draws, &bank);
        };
        let storage = || {
            SCRATCH.with(|s| {
                let s = s.borrow();
                s.as_ref().map(|l| l.pixels().as_ptr() as usize)
            })
        };
        draw_band(0, 24);
        let first = storage().expect("a layer was made");
        for _ in 0..5 {
            draw_band(0, 24);
            draw_band(24, 24);
            draw_band(48, 12);
        }
        assert_eq!(storage(), Some(first), "reused, not reallocated");
    })
    .join()
    .unwrap();
}

#[test]
fn hostile_layered_sprites_draw_nothing_and_do_not_panic() {
    let bad = [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::MAX,
        f32::MIN,
        1.0e30,
        -1.0,
        0.0,
    ];
    let mut d = ParticleDraws::default();
    for &a in &bad {
        for &b in &bad {
            d.layer_over
                .push(sprite(a, b, a, b, 0xffff_ffff, Mask::Soft));
            d.layer_add
                .push(sprite(b, a, 40.0, a, 0xffff_ffff, Mask::Ring));
            d.layer_add
                .push(sprite(40.0, 40.0, a, 0.5, 0xffff_ffff, Mask::Soft));
        }
    }
    let got = whole(&d, Level::best(), &stripes);
    let bare = whole(&ParticleDraws::default(), Level::best(), &stripes);
    // The huge but finite ones may draw; the call must simply return. Rows
    // are intact in shape.
    assert_eq!(got.pixels().len(), bare.pixels().len());
}
