use crate::step::{FixedStepper, MAX_STEPS_PER_FRAME, Simulation, interpolate};
use crate::test_util::*;
use mscent_math::Vec3;
use mscent_math::rng::Rng;

const FRAME_60HZ_NS: u64 = 16_666_667;

#[test]
fn one_second_is_exactly_120_steps() {
    let mut s = FixedStepper::new();
    let total: u32 = (0..1000).map(|_| s.advance(1_000_000)).sum();
    assert_eq!(total, 120);
}

#[test]
fn uneven_frames_give_the_same_total_as_one_long_frame() {
    let mut a = FixedStepper::new();
    let mut rng = Rng::new(3);
    let (mut steps, mut ns) = (0u32, 0u64);
    for _ in 0..100 {
        let dt = u64::from(rng.below(1_000_000));
        ns += dt;
        steps += a.advance(dt);
    }
    let mut b = FixedStepper::new();
    assert_eq!(steps, b.advance(ns));
    assert_eq!(a, b);
}

#[test]
fn stall_is_capped_and_alpha_stays_in_range() {
    let mut s = FixedStepper::new();
    assert_eq!(s.advance(u64::MAX), MAX_STEPS_PER_FRAME);
    assert!((0.0..1.0).contains(&s.alpha()));
}

#[test]
fn interpolation_blends_between_steps() {
    let (a, b) = (v(0.0, 0.0, 0.0), v(2.0, 0.0, 0.0));
    assert_eq!(interpolate(a, b, 0.25), v(0.5, 0.0, 0.0));
    assert_eq!(interpolate(a, b, 5.0), b);
    let mut sim = Simulation::new(v(0.0, 0.0, 0.0), 1.0);
    sim.body.vel = v(120.0, 0.0, 0.0);
    // 1.5 steps worth of time: one step runs, half a step is left over.
    sim.update(&room(), 12_500_000, Vec3::ZERO);
    assert_eq!(sim.body.pos, v(1.0, 0.0, 0.0));
    assert!((sim.render_pos().x - 0.5).abs() < 1e-3);
}

fn run(seed: u64) -> Vec<[u32; 3]> {
    let bsp = room_with_block();
    let mut rng = Rng::new(seed);
    let mut sim = Simulation::new(v(-10.0, -3.0, 2.0), 0.75);
    let mut trace = Vec::new();
    for _ in 0..600 {
        let accel = v(
            rng.range_f32(-80.0, 80.0),
            rng.range_f32(-80.0, 80.0),
            rng.range_f32(-80.0, 80.0),
        );
        let frame = FRAME_60HZ_NS + u64::from(rng.below(4_000_000));
        sim.update(&bsp, frame, accel);
        let p = sim.render_pos();
        trace.push([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]);
    }
    trace
}

#[test]
fn replayed_inputs_reproduce_positions_bit_exactly() {
    assert_eq!(run(99), run(99));
    assert_ne!(run(99), run(100));
}

#[test]
fn simulated_body_never_enters_solid() {
    let bsp = room_with_block();
    let mut rng = Rng::new(5);
    let mut sim = Simulation::new(v(-10.0, -3.0, 2.0), 0.75);
    for _ in 0..3000 {
        let accel = v(
            rng.range_f32(-300.0, 300.0),
            rng.range_f32(-300.0, 300.0),
            rng.range_f32(-300.0, 300.0),
        );
        sim.update(&bsp, 8_333_333, accel);
        let p = sim.body.pos;
        assert!(block_room_clearance(p) >= 0.75 - 1.0e-4, "{p:?}");
    }
}
