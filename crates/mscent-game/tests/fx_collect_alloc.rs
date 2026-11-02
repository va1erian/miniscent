//! Turning live effects into draw lists allocates nothing, because the lists
//! start at the size of the fullest frame. Its own binary with one test: the counter is global.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_game::fx::combat::{CombatFx, DRONE_TINT, Impact, ShotFx};
use mscent_math::{Vec3, Vec3d};
use mscent_render::View;
use mscent_world::materials::METAL;
use mscent_world::{Brush, Op, compile};

struct Counting;

static CALLS: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every method forwards to `System` with the same arguments, so the
// allocator contract is exactly System's; the counter is an atomic.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.alloc(l) }
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        // SAFETY: forwarded unchanged.
        unsafe { System.dealloc(p, l) }
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        CALLS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: forwarded unchanged.
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

#[test]
fn collecting_a_busy_frame_allocates_nothing_from_the_first_one() {
    let room = Brush::cuboid(
        Vec3d::new(-40.0, -40.0, -40.0),
        Vec3d::new(40.0, 40.0, 40.0),
        Op::Subtract,
        0,
    )
    .expect("a room");
    let bsp = compile(&[room]).expect("compiles");
    let view = View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, 640, 360).expect("a camera");
    let mut fx = CombatFx::new();
    let mut draws = CombatFx::new_draws();
    let ahead = Vec3::new(0.0, 5.0, 0.0);
    fx.drone_explosion(ahead, DRONE_TINT);
    fx.shot(&ShotFx {
        by_player: true,
        muzzle: Vec3::ZERO,
        end: ahead,
        normal: Vec3::new(0.0, -1.0, 0.0),
        impact: Impact::Drone,
    });
    fx.advance(16_666_667, &bsp);
    let before = CALLS.load(Ordering::Relaxed);
    fx.collect(&cam, None, None, &mut draws);
    let calls = CALLS.load(Ordering::Relaxed) - before;
    assert!(!draws.is_empty());
    assert_eq!(calls, 0);

    // A wall covered in scorch marks, more than the ring holds: a frame with
    // every mark in view still allocates nothing.
    for i in 0..70 {
        let end = Vec3::new(
            (i % 10) as f32 * 3.0 - 15.0,
            39.9,
            (i / 10) as f32 * 3.0 - 10.0,
        );
        fx.shot(&ShotFx {
            by_player: true,
            muzzle: Vec3::ZERO,
            end,
            normal: Vec3::new(0.0, -1.0, 0.0),
            impact: Impact::Surface {
                material: METAL,
                tint: 0xff80_8080,
            },
        });
    }
    for _ in 0..10 {
        fx.advance(16_666_667, &bsp);
    }
    assert_eq!(fx.decals(), 64, "the ring is full");
    let before = CALLS.load(Ordering::Relaxed);
    fx.collect(&cam, None, None, &mut draws);
    let calls = CALLS.load(Ordering::Relaxed) - before;
    assert!(draws.decals.len() > 30, "{} in view", draws.decals.len());
    assert_eq!(calls, 0);

    // A crowd of explosions that overruns the budget, so big sprites are moved
    // to the half-res layer: the move and the cut allocate nothing either, and
    // do so every frame the layer is held.
    let mut fx = CombatFx::new();
    for i in 0..40 {
        let spread = i as f32 / 40.0 - 0.5;
        let at = Vec3::new(spread * 16.0, 8.0 + spread * 4.0, spread * 6.0);
        fx.drone_explosion(at, DRONE_TINT);
    }
    for _ in 0..12 {
        fx.advance(16_666_667, &bsp);
    }
    for frame in 0..4 {
        let before = CALLS.load(Ordering::Relaxed);
        fx.collect(&cam, None, None, &mut draws);
        let calls = CALLS.load(Ordering::Relaxed) - before;
        let layered = draws.layer_over.len() + draws.layer_add.len();
        assert!(
            layered > 20,
            "frame {frame}: {layered} sprites in the layer"
        );
        assert_eq!(calls, 0, "frame {frame}");
    }
}
