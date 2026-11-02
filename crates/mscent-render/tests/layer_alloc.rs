//! Drawing layered sprites allocates nothing after the first frame: the band's
//! half-res scratch is made once and reused. Its own binary with one test: the
//! counter is global.

// A counting allocator is an `unsafe impl` by definition; every call forwards to
// `System`.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use mscent_render::particles::{Mask, ParticleDraws, SpriteBank, SpriteDraw, draw_particles};
use mscent_render::{Framebuffer, TileRenderer};

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

fn layered() -> ParticleDraws {
    let s = |x, tint, mask| SpriteDraw {
        x,
        y: 60.0,
        radius: 70.0,
        inv_z: 0.5,
        tint,
        mask,
    };
    let mut d = ParticleDraws::with_capacity(8, 0);
    d.layer_over.push(s(80.0, 0xc040_6080, Mask::Puff(1)));
    d.layer_add.push(s(120.0, 0xff40_3020, Mask::Soft));
    d
}

#[test]
fn layered_sprites_allocate_nothing_after_the_first_frame() {
    let (draws, bank) = (layered(), SpriteBank::new());
    // Odd sizes, so the layer's last row and column are partial.
    let mut fb = Framebuffer::new(201, 121).expect("a target");
    draw_particles(&mut fb, &draws, &bank);
    let before = CALLS.load(Ordering::Relaxed);
    for _ in 0..5 {
        draw_particles(&mut fb, &draws, &bank);
    }
    assert_eq!(CALLS.load(Ordering::Relaxed) - before, 0, "whole frame");

    // One worker draws every band on this thread, so its scratch is the one
    // that matters; bands of different sizes (24, 24, ..., 1 rows) reshape it.
    let mut tiles = TileRenderer::new(201, 121, 24, 1).expect("tiles");
    let mut out = Framebuffer::new(201, 121).expect("a target");
    let frame = |tiles: &mut TileRenderer, out: &mut Framebuffer| {
        tiles
            .render(out, &|band| draw_particles(band, &draws, &bank))
            .expect("same size");
    };
    frame(&mut tiles, &mut out);
    let before = CALLS.load(Ordering::Relaxed);
    for _ in 0..5 {
        frame(&mut tiles, &mut out);
    }
    assert_eq!(CALLS.load(Ordering::Relaxed) - before, 0, "banded frame");
}
