use super::*;

fn all_masks() -> Vec<Mask> {
    let mut v = vec![Mask::Soft, Mask::Ring];
    v.extend((0..4).map(Mask::Puff));
    v.extend((0..8).map(Mask::Fire));
    v
}

#[test]
fn bank_is_about_300_kb_and_exactly_the_documented_size() {
    let bank = SpriteBank::new();
    assert_eq!(bank.data.len(), 14 * 21824);
    assert!((290_000..320_000).contains(&bank.data.len()));
}

#[test]
fn bank_is_reproducible() {
    assert_eq!(SpriteBank::new().data, SpriteBank::new().data);
}

#[test]
fn every_mask_has_every_mip_of_the_right_size() {
    let bank = SpriteBank::new();
    for mask in all_masks() {
        for level in 0..MIPS {
            let (bytes, side) = bank.mip(mask, level);
            assert_eq!(side, 8 << level);
            assert_eq!(bytes.len(), side * side, "{mask:?} level {level}");
        }
    }
}

#[test]
fn indices_wrap_and_are_all_distinct_slots() {
    assert_eq!(Mask::Puff(4).index(), Mask::Puff(0).index());
    assert_eq!(Mask::Puff(255).index(), Mask::Puff(3).index());
    assert_eq!(Mask::Fire(8).index(), Mask::Fire(0).index());
    assert_eq!(Mask::Fire(255).index(), Mask::Fire(7).index());
    let mut seen: Vec<usize> = all_masks().into_iter().map(Mask::index).collect();
    seen.sort_unstable();
    assert_eq!(seen, (0..14).collect::<Vec<_>>());
}

#[test]
fn level_past_the_end_uses_the_largest_mip() {
    let bank = SpriteBank::new();
    assert_eq!(bank.mip(Mask::Soft, 99).1, 128);
}

#[test]
fn shapes_look_like_what_they_are_named() {
    let bank = SpriteBank::new();
    let at = |m: Mask, x: usize, y: usize| bank.mip(m, 4).0[y * 128 + x];
    // Soft: bright in the middle, nothing in the corner.
    assert!(at(Mask::Soft, 64, 64) > 240);
    assert_eq!(at(Mask::Soft, 0, 0), 0);
    // Ring: hollow centre, bright on the band.
    assert_eq!(at(Mask::Ring, 64, 64), 0);
    assert!(at(Mask::Ring, 64 + 45, 64) > 200);
    // Every mask is empty in its corners, so a box never shows its edge.
    for mask in all_masks() {
        for (x, y) in [(0, 0), (127, 0), (0, 127), (127, 127)] {
            assert_eq!(at(mask, x, y), 0, "{mask:?}");
        }
    }
}

#[test]
fn puffs_differ_and_fire_frames_grow_then_fade() {
    let bank = SpriteBank::new();
    assert_ne!(bank.mip(Mask::Puff(0), 4).0, bank.mip(Mask::Puff(1), 4).0);
    let sum = |f: u8| -> u64 {
        bank.mip(Mask::Fire(f), 4)
            .0
            .iter()
            .map(|&b| u64::from(b))
            .sum()
    };
    assert!(sum(0) > 0);
    assert!(sum(7) < sum(3), "late frames are dimmer");
    assert_ne!(
        bank.mip(Mask::Fire(1), 4).0,
        bank.mip(Mask::Fire(2), 4).0,
        "frames differ"
    );
}

#[test]
fn mips_keep_the_average_coverage() {
    let bank = SpriteBank::new();
    let mean = |level: usize| -> f64 {
        let (b, _) = bank.mip(Mask::Soft, level);
        b.iter().map(|&v| f64::from(v)).sum::<f64>() / b.len() as f64
    };
    for level in 0..4 {
        assert!(
            (mean(level) - mean(level + 1)).abs() < 1.5,
            "level {level}: {} vs {}",
            mean(level),
            mean(level + 1)
        );
    }
}
