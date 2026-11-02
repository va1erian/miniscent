use super::*;
use mscent_render::particles::Mask;

const W: f32 = 640.0;
const H: f32 = 360.0;
const SCREEN: f32 = W * H;

/// Covers the whole screen, as the design's worst frame does.
fn full_screen(blend_tint: u32) -> SpriteDraw {
    SpriteDraw {
        x: W / 2.0,
        y: H / 2.0,
        radius: 400.0,
        inv_z: 0.5,
        tint: blend_tint,
        mask: Mask::Soft,
    }
}

fn tag(tier: Tier, key: usize) -> Tag {
    Tag {
        tier,
        key: key as u8,
    }
}

/// `n` full-screen sprites with keys spread over the byte range.
fn fill(n: usize, tier: Tier, draws: &mut ParticleDraws, tags: &mut Tags) {
    let mut sink = Sink {
        out: draws,
        tags,
        width: W,
        height: H,
    };
    for i in 0..n {
        let key = i * 256 / n;
        sink.sprite(full_screen(0x40ff_ffff), Blend::Add, tag(tier, key));
    }
}

fn fitted(budget: f32, draws: &mut ParticleDraws, tags: &Tags) -> Keep {
    let keep = Tally::of(draws, tags, None, W, H).fit(budget);
    thin(draws, tags, keep);
    keep
}

#[test]
fn a_hundred_full_screen_sprites_stay_within_ten_percent_of_budget() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    fill(100, Tier::Normal, &mut draws, &mut tags);
    let before = cost_of(&draws, None, W, H);
    let budget = Limits::of(Quality::High).screens * SCREEN;
    assert!(before > 10.0 * budget, "starts far over: {before}");
    fitted(budget, &mut draws, &tags);
    let after = cost_of(&draws, None, W, H);
    assert!(after <= budget * 1.1, "{after} against {budget}");
    assert!(after >= budget * 0.7, "and spends it: {after} of {budget}");
}

#[test]
fn essential_is_untouched_however_far_over_it_goes() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    fill(50, Tier::Essential, &mut draws, &mut tags);
    fill(50, Tier::Normal, &mut draws, &mut tags);
    let keep = fitted(SCREEN, &mut draws, &tags);
    assert_eq!(keep.normal, 0);
    assert_eq!(draws.add.len(), 50);
    assert!(draws.add.iter().all(|s| s.tint == 0x40ff_ffff));
}

#[test]
fn cosmetic_goes_before_normal() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    fill(4, Tier::Normal, &mut draws, &mut tags);
    fill(100, Tier::Cosmetic, &mut draws, &mut tags);
    let keep = fitted(5.0 * SCREEN, &mut draws, &tags);
    assert_eq!(keep.normal, Keep::default().normal, "normal all fits");
    assert!(keep.cosmetic < 256, "{keep:?}");
}

#[test]
fn nothing_is_thinned_or_faded_under_budget() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    fill(2, Tier::Cosmetic, &mut draws, &mut tags);
    assert_eq!(fitted(5.0 * SCREEN, &mut draws, &tags), Keep::default());
    assert_eq!(draws.add.len(), 2);
    assert!(draws.add.iter().all(|s| s.tint == 0x40ff_ffff));
}

#[test]
fn a_particle_fades_as_the_cut_nears_it_and_the_same_ones_survive() {
    let keep = Keep {
        normal: 100,
        cosmetic: 100,
    };
    let at = |key| opacity(tag(Tier::Normal, key), keep);
    assert_eq!(at(100), None);
    assert_eq!(at(99), Some(8), "one key clear of the cut");
    assert_eq!(at(84), Some(128), "halfway through the fade");
    assert_eq!(at(68), Some(256), "past the fade");
    assert_eq!(at(0), Some(256));
}

#[test]
fn the_cut_falls_at_once_and_climbs_back_slowly() {
    let mut keep = Keep::default();
    let low = Keep {
        normal: 10,
        cosmetic: 0,
    };
    keep.follow(low);
    assert_eq!(keep, low);
    keep.follow(Keep::default());
    assert_eq!(keep.normal, 10 + KEEP_RISE);
    assert_eq!(keep.cosmetic, KEEP_RISE);
    for _ in 0..100 {
        keep.follow(Keep::default());
    }
    assert_eq!(keep, Keep::default());
}

#[test]
fn off_screen_and_hostile_sprites_are_dropped_and_cost_nothing() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    let mut sink = Sink {
        out: &mut draws,
        tags: &mut tags,
        width: W,
        height: H,
    };
    let t = tag(Tier::Normal, 0);
    for s in [
        SpriteDraw {
            x: -1000.0,
            ..full_screen(1)
        },
        SpriteDraw {
            x: f32::NAN,
            ..full_screen(1)
        },
        SpriteDraw {
            radius: f32::NAN,
            ..full_screen(1)
        },
        SpriteDraw {
            radius: -5.0,
            ..full_screen(1)
        },
    ] {
        sink.sprite(s, Blend::Over, t);
    }
    assert!(draws.is_empty());
    assert_eq!(
        Tally::of(&draws, &tags, None, W, H).fit(0.0),
        Keep::default()
    );
}

#[test]
fn small_sprites_are_costed_by_count_as_well_as_area() {
    let small = SpriteDraw {
        radius: 1.0,
        ..full_screen(1)
    };
    let c = sprite_cost(&small, 1.0, None, W, H);
    assert!(c > 4.0 * 10.0, "setup dominates a 2 px sprite: {c}");
}

#[test]
fn a_quality_spends_what_its_row_says() {
    assert_eq!(Limits::of(Quality::High).screens, 5.0);
    assert_eq!(Limits::of(Quality::Medium).density, 0.7);
    assert_eq!(Limits::of(Quality::Low).screens, 1.5);
}
