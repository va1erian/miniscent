use super::*;

fn controller() -> ResolutionController {
    ResolutionController::new(ResolutionConfig::default()).unwrap()
}

/// Cost model: proportional to pixel count, `full_us` at full scale.
fn cost(full_us: u32, scale: u32) -> u32 {
    (u64::from(full_us) * u64::from(scale) * u64::from(scale) / 1_000_000) as u32
}

#[test]
fn rejects_bad_config() {
    let ok = ResolutionConfig::default();
    for bad in [
        ResolutionConfig { budget_us: 0, ..ok },
        ResolutionConfig {
            step_permille: 0,
            ..ok
        },
        ResolutionConfig {
            min_permille: 0,
            ..ok
        },
        ResolutionConfig {
            min_permille: 1001,
            ..ok
        },
        ResolutionConfig {
            headroom_percent: 0,
            ..ok
        },
        ResolutionConfig {
            headroom_percent: 101,
            ..ok
        },
    ] {
        assert!(ResolutionController::new(bad).is_none(), "{bad:?}");
    }
}

#[test]
fn stays_at_full_when_under_budget() {
    let mut c = controller();
    for _ in 0..200 {
        assert_eq!(c.update(10_000), FULL);
    }
}

#[test]
fn drops_when_cost_exceeds_budget() {
    let mut c = controller();
    let s = c.update(40_000);
    assert!(s < FULL, "{s}");
    // 40 ms at full needs about 0.64 to fit 16.6 ms; the drop must be
    // at least that deep after rounding to a step.
    assert!(s <= 750, "{s}");
}

#[test]
fn never_goes_below_minimum() {
    let mut c = controller();
    for _ in 0..50 {
        c.update(1_000_000);
    }
    assert_eq!(c.scale_permille(), ResolutionConfig::default().min_permille);
}

#[test]
fn converges_under_budget_and_recovers_when_load_falls() {
    let mut c = controller();
    let heavy = 45_000;
    for _ in 0..20 {
        let s = c.scale_permille();
        c.update(cost(heavy, s));
    }
    let settled = c.scale_permille();
    assert!(settled < FULL);
    assert!(cost(heavy, settled) <= ResolutionConfig::default().budget_us);
    // The scene gets cheap again: the scale climbs back to full.
    let light = 6_000;
    for _ in 0..1000 {
        let s = c.scale_permille();
        c.update(cost(light, s));
    }
    assert_eq!(c.scale_permille(), FULL);
}

#[test]
fn does_not_flap_when_the_next_step_would_not_fit() {
    let mut c = controller();
    let heavy = 30_000;
    for _ in 0..20 {
        let s = c.scale_permille();
        c.update(cost(heavy, s));
    }
    let settled = c.scale_permille();
    let mut changes = 0;
    for _ in 0..1000 {
        let before = c.scale_permille();
        c.update(cost(heavy, before));
        changes += usize::from(c.scale_permille() != before);
    }
    assert_eq!(changes, 0, "scale kept changing around {settled}");
}

#[test]
fn recovery_waits_for_consecutive_cheap_frames() {
    let mut c = controller();
    c.update(40_000);
    let low = c.scale_permille();
    for _ in 0..29 {
        c.update(1_000);
    }
    assert_eq!(c.scale_permille(), low);
    c.update(16_000); // not cheap enough for a step: resets the streak
    for _ in 0..29 {
        c.update(1_000);
    }
    assert_eq!(c.scale_permille(), low);
    c.update(1_000);
    assert!(c.scale_permille() > low);
}

#[test]
fn internal_size_is_scaled_and_bounded() {
    let mut c = controller();
    assert_eq!(c.internal_size(1280, 720), (1280, 720));
    c.update(1_000_000);
    let (w, h) = c.internal_size(1280, 720);
    assert_eq!((w, h), (320, 180));
    assert_eq!(c.internal_size(1, 1), (1, 1));
    assert_eq!(c.internal_size(0, 0), (1, 1));
}

#[test]
fn integer_upscale_replicates_pixels() {
    let src = [1, 2, 3, 4];
    let mut dst = [0u32; 16];
    assert!(upscale_nearest(&src, 2, 2, &mut dst, 4, 4));
    assert_eq!(dst, [1, 1, 2, 2, 1, 1, 2, 2, 3, 3, 4, 4, 3, 3, 4, 4]);
}

#[test]
fn non_integer_upscale_is_nearest_and_covers_every_source_pixel() {
    let src: Vec<u32> = (0..9).collect();
    let (dw, dh) = (7u32, 5u32);
    let mut dst = vec![u32::MAX; (dw * dh) as usize];
    assert!(upscale_nearest(&src, 3, 3, &mut dst, dw, dh));
    for y in 0..dh {
        for x in 0..dw {
            let sx = (2 * x + 1) * 3 / (2 * dw);
            let sy = (2 * y + 1) * 3 / (2 * dh);
            assert_eq!(dst[(y * dw + x) as usize], sy * 3 + sx, "({x},{y})");
        }
    }
    for v in 0..9 {
        assert!(dst.contains(&v));
    }
}

#[test]
fn identity_and_hostile_inputs() {
    let src: Vec<u32> = (0..12).collect();
    let mut same = vec![0; 12];
    assert!(upscale_nearest(&src, 4, 3, &mut same, 4, 3));
    assert_eq!(same, src);
    let mut dst = vec![7; 4];
    assert!(!upscale_nearest(&src, 4, 3, &mut dst, 2, 3), "bad dst len");
    assert!(!upscale_nearest(&src, 5, 3, &mut dst, 2, 2), "bad src len");
    assert!(!upscale_nearest(&[], 0, 0, &mut dst, 2, 2));
    assert!(!upscale_nearest(&src, 4, 3, &mut [], 0, 0));
    assert_eq!(dst, [7; 4], "failed calls write nothing");
}

#[test]
fn downscale_also_works() {
    let src: Vec<u32> = (0..16).collect();
    let mut dst = vec![0; 4];
    assert!(upscale_nearest(&src, 4, 4, &mut dst, 2, 2));
    assert_eq!(dst, [5, 7, 13, 15]);
}

#[test]
fn huge_step_does_not_overflow() {
    let cfg = ResolutionConfig {
        step_permille: u32::MAX,
        ..ResolutionConfig::default()
    };
    let mut c = ResolutionController::new(cfg).unwrap();
    c.update(cfg.budget_us + 1);
    for _ in 0..100 {
        assert!(c.update(1) <= FULL);
    }
}
