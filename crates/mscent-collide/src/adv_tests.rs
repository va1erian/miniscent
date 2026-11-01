//! Adversarial regression tests: tunnelling, sticking and degenerate input.

use crate::slide::slide_move;
use crate::step::Body;
use crate::sweep::sweep_sphere;
use crate::test_util::*;
use mscent_math::Vec3;
use mscent_math::rng::Rng;
use mscent_world::{Brush, Bsp, Op, compile};

fn thin_slab_room() -> Bsp {
    // 0.1 thick wall at x in [5, 5.02] spanning the room.
    let slab = Brush::cuboid(vd(5.0, -19.0, -19.0), vd(5.1, 19.0, 19.0), Op::Add, 1).unwrap();
    compile(&[room_brush(), slab]).unwrap()
}

#[test]
fn fast_sweeps_cannot_tunnel_a_thin_wall() {
    let bsp = thin_slab_room();
    let mut rng = Rng::new(11);
    for _ in 0..2000 {
        let from = v(
            rng.range_f32(-15.0, 4.0),
            rng.range_f32(-15.0, 15.0),
            rng.range_f32(-15.0, 15.0),
        );
        let to = v(
            rng.range_f32(6.0, 19.0),
            rng.range_f32(-15.0, 15.0),
            rng.range_f32(-15.0, 15.0),
        );
        let r = rng.range_f32(0.0, 1.0);
        let t = sweep_sphere(&bsp, from, to, r);
        assert!(t.hit && t.end.x < 5.0, "{from:?} -> {to:?} r {r}: {t:?}");
    }
}

#[test]
fn slides_never_tunnel_or_embed_with_big_steps() {
    let bsp = room_with_block();
    let mut rng = Rng::new(21);
    let r = 0.6;
    for _ in 0..3000 {
        let pos = v(
            rng.range_f32(-19.0, 19.0),
            rng.range_f32(-19.0, 19.0),
            rng.range_f32(-19.0, 19.0),
        );
        if block_room_clearance(pos) < r {
            continue;
        }
        let vel = v(
            rng.range_f32(-900.0, 900.0),
            rng.range_f32(-900.0, 900.0),
            rng.range_f32(-900.0, 900.0),
        );
        let dt = rng.range_f32(0.001, 0.5);
        let res = slide_move(&bsp, pos, vel, dt, r);
        assert!(
            block_room_clearance(res.pos) >= r - 1.0e-3,
            "{pos:?} {vel:?} {dt}: {res:?}"
        );
    }
}

#[test]
fn body_in_a_corner_can_always_leave_it() {
    let bsp = room();
    let mut b = Body::new(v(0.0, 0.0, 0.0), 0.5);
    for _ in 0..600 {
        b.vel = v(100.0, 100.0, 100.0);
        b.step(&bsp, Vec3::ZERO);
    }
    let pinned = b.pos;
    assert!(
        pinned.x > 19.0 && pinned.y > 19.0 && pinned.z > 19.0,
        "{pinned:?}"
    );
    for _ in 0..120 {
        b.vel = v(-5.0, -5.0, -5.0);
        b.step(&bsp, Vec3::ZERO);
    }
    assert!(
        b.pos.x < pinned.x - 4.0 && b.pos.y < pinned.y - 4.0 && b.pos.z < pinned.z - 4.0,
        "{:?}",
        b.pos
    );
}

#[test]
fn body_on_block_edge_and_vertex_keeps_moving() {
    let bsp = room_with_block();
    // Slide along the top edge of the block toward its corner and past it.
    let mut b = Body::new(v(-3.0, 4.2, 4.2), 0.5);
    let mut moved = 0.0;
    for _ in 0..240 {
        b.vel = v(4.0, -1.0, -1.0);
        let before = b.pos;
        b.step(&bsp, Vec3::ZERO);
        moved += (b.pos - before).length();
        assert!(block_room_clearance(b.pos) >= 0.5 - 1.0e-4, "{:?}", b.pos);
    }
    assert!(moved > 2.0, "stuck after {moved}");
}

#[test]
fn resting_under_gravity_neither_sinks_nor_jitters() {
    for bsp in [room(), room_with_ramp()] {
        let mut b = Body::new(v(-5.0, -10.0, 5.0), 0.5);
        let g = v(0.0, -9.8, 0.0);
        for _ in 0..1200 {
            b.step(&bsp, g);
        }
        let rest = b.pos;
        for _ in 0..1200 {
            b.step(&bsp, g);
            assert!((b.pos - rest).length() < 1.0e-2, "{:?} vs {rest:?}", b.pos);
        }
    }
}

#[test]
fn ramp_bottom_crease_and_wall_never_embeds() {
    let bsp = room_with_ramp();
    let s = std::f32::consts::FRAC_1_SQRT_2;
    let mut rng = Rng::new(31);
    let mut b = Body::new(v(0.0, 0.0, 12.0), 0.5);
    for _ in 0..20000 {
        let a = v(
            rng.range_f32(-300.0, 300.0),
            rng.range_f32(-300.0, 300.0),
            rng.range_f32(-300.0, 300.0),
        );
        b.step(&bsp, a);
        let p = b.pos;
        assert!(p.x.is_finite() && p.y.is_finite() && p.z.is_finite());
        if p.x.abs() < 10.0 && p.y.abs() < 10.0 && p.z > -9.5 {
            assert!((p.z - p.x) * s >= 0.5 - 1.0e-3, "{p:?}");
        }
        assert!(
            p.x.abs() <= 19.5 && p.y.abs() <= 19.5 && p.z.abs() <= 19.5,
            "{p:?}"
        );
    }
}

#[test]
fn extreme_inputs_do_not_panic() {
    let bsp = room_with_block();
    let big = f32::MAX;
    for (pos, vel, dt, r) in [
        (v(-10.0, 0.0, 0.0), v(big, big, big), 1.0, 0.5),
        (v(-10.0, 0.0, 0.0), v(1.0, 0.0, 0.0), big, 0.5),
        (v(-10.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, big),
        (v(1e30, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, 0.5),
        (v(-10.0, 0.0, 0.0), v(1.0, 0.0, 0.0), -1.0, 0.5),
        (v(-10.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, 0.0),
        (v(-10.0, 0.0, 0.0), v(1.0, 0.0, 0.0), 1.0, -1.0),
    ] {
        let r = slide_move(&bsp, pos, vel, dt, r);
        assert!(r.pos.is_finite() && r.vel.is_finite());
    }
}

/// Solid under `z = |x|` (a 90 degree valley along Y) for `|x|, |y| <= 10`.
fn valley() -> Bsp {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let half = |sign: f64| {
        let planes = [
            mscent_math::DPlane::new(vd(sign, 0.0, 0.0), 10.0 * sign.abs()),
            mscent_math::DPlane::new(vd(-sign, 0.0, 0.0), 0.0),
            mscent_math::DPlane::new(vd(0.0, 1.0, 0.0), 10.0),
            mscent_math::DPlane::new(vd(0.0, -1.0, 0.0), 10.0),
            mscent_math::DPlane::new(vd(0.0, 0.0, -1.0), 10.0),
            mscent_math::DPlane::new(vd(-s * sign, 0.0, s), 0.0),
        ];
        Brush::new(&planes, Op::Add, 1).unwrap()
    };
    compile(&[room_brush(), half(1.0), half(-1.0)]).unwrap()
}

#[test]
fn valley_crease_holds_a_body_and_lets_it_run_along_it() {
    let bsp = valley();
    let mut b = Body::new(v(0.0, 0.0, 6.0), 0.5);
    let g = v(0.0, 0.0, -9.8);
    for _ in 0..1200 {
        b.step(&bsp, g);
    }
    let rest = b.pos;
    assert!((rest.x).abs() < 0.05, "{rest:?}");
    for _ in 0..240 {
        b.step(&bsp, g + v(0.0, 20.0, 0.0));
        if b.pos.y < 9.0 {
            assert!(b.pos.z >= 0.5 * 2f32.sqrt() - 0.05, "{:?}", b.pos);
        }
    }
    assert!(b.pos.y > rest.y + 3.0, "stuck in the crease: {:?}", b.pos);
}
