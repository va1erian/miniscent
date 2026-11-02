//! Spawning and ticking particles allocate nothing after `Fx::new`. The count
//! is taken with a counting global allocator, as in `mscent-render`'s
//! `frame_alloc.rs`. One test function only: the counter is global, so two
//! tests running on two threads would count each other.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_game::fx::{
    Blend, ClassId, Collide, EffectArgs, Emitter, Fx, ParticleClass, Prim, Shape, Tier, TintSource,
    Trail,
};
use mscent_math::{Vec3, Vec3d};
use mscent_world::{Brush, Bsp, Op, compile};

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

const fn class(collide: Collide, capacity: u16, tier: Tier) -> ParticleClass {
    ParticleClass {
        prim: Prim::Spark,
        blend: Blend::Add,
        tier,
        gravity: 2.0,
        drag: 0.5,
        collide,
        ramp: &[0xFFFF_FFFF],
        size: (0.1, 0.0),
        stretch: 0.0,
        capacity,
        trail: None,
    }
}

const fn emitter(class: u8, count: u16, shape: Shape, speed: f32, delay: f32) -> Emitter {
    Emitter {
        class: ClassId(class),
        count: (count, count),
        shape,
        speed: (speed, speed),
        life: (1.0, 3.0),
        delay,
        tint: TintSource::Surface,
    }
}

static TABLE: [ParticleClass; 5] = [
    class(Collide::None, 500, Tier::Normal),
    class(Collide::Die, 500, Tier::Normal),
    class(Collide::Bounce { keep: 0.6 }, 500, Tier::Essential),
    ParticleClass {
        trail: Some(Trail {
            class: ClassId(0),
            interval: 0.05,
        }),
        ..class(Collide::None, 100, Tier::Normal)
    },
    class(Collide::None, 8, Tier::Essential),
];

static CALM: [Emitter; 6] = [
    emitter(0, 200, Shape::Sphere(2.0), 1.0, 0.0),
    emitter(1, 200, Shape::Cone { half_angle: 0.7 }, 1.0, 0.0),
    emitter(2, 200, Shape::Hemisphere, 1.0, 0.0),
    emitter(3, 50, Shape::Volume, 1.0, 0.0),
    emitter(4, 200, Shape::Point, 1.0, 0.0), // overflows an Essential pool of 8
    emitter(0, 100, Shape::Segment, 1.0, 0.2),
];

fn room() -> Bsp {
    let room = Brush::cuboid(
        Vec3d::new(-60.0, -60.0, -60.0),
        Vec3d::new(60.0, 60.0, 60.0),
        Op::Subtract,
        0,
    )
    .expect("room brush");
    compile(&[room]).expect("room compiles")
}

fn args() -> EffectArgs {
    EffectArgs {
        pos: Vec3::ZERO,
        dir: Vec3::new(0.0, 1.0, 0.0),
        end: Vec3::new(3.0, 0.0, 0.0),
        scale: 1.0,
        surface: 0x808080,
        volume: Some(mscent_math::Aabb::new(
            Vec3::new(-2.0, -2.0, -2.0),
            Vec3::new(2.0, 2.0, 2.0),
        )),
    }
}

#[test]
fn spawn_and_tick_allocate_nothing() {
    let bsp = room();
    let mut fx = Fx::new(&TABLE, 42);
    let args = args();

    let before = CALLS.load(Ordering::Relaxed);
    for _ in 0..20 {
        fx.spawn(&CALM, &args);
        for _ in 0..30 {
            fx.advance(16_666_667, &bsp);
        }
    }
    let mut seen = 0;
    fx.for_each(|_| seen += 1);
    fx.clear();
    let allocations = CALLS.load(Ordering::Relaxed) - before;

    // Bouncing off the walls costs a surface query each time; it must not allocate.
    static HIT: [Emitter; 1] = [emitter(2, 8, Shape::Sphere(1.0), 3000.0, 0.0)];
    let before_hits = CALLS.load(Ordering::Relaxed);
    fx.spawn(&HIT, &args);
    let mut surface_queries = 0;
    for _ in 0..60 {
        fx.advance(16_666_667, &bsp);
        surface_queries += fx.last_tick_collision().sweeps;
    }
    assert!(surface_queries > 0, "no particle reached a wall");
    let bounce_allocations = CALLS.load(Ordering::Relaxed) - before_hits;
    assert_eq!(bounce_allocations, 0, "wall bounces allocated");
    assert!(seen > 0, "nothing was simulated");
    assert_eq!(
        allocations, 0,
        "spawn, advance, for_each and clear allocated"
    );
}
