//! The hidden `--slow-play-ms`: every frame takes longer, to reproduce a slow
//! machine and see that a slow game does not slow the UI.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Framebuffer, VoxelChange};

use super::worker::{Request, Scene, Surface};
use crate::build::Built;

/// Extra delay per rendered frame, in ms (the hidden `--slow-play-ms`).
static SLOW_MS: AtomicU64 = AtomicU64::new(0);

/// Makes every scene created from now on take `ms` longer per frame, to
/// reproduce a slow machine.
pub fn set_slow_ms(ms: u64) {
    SLOW_MS.store(ms, Ordering::Relaxed);
}

pub fn slow_ms() -> u64 {
    SLOW_MS.load(Ordering::Relaxed)
}

/// Delays every frame, wrapping a scene (the `--slow-play-ms` flag).
pub struct Slow {
    pub inner: Box<dyn Scene>,
    pub delay: Duration,
}

impl Scene for Slow {
    fn restart(&mut self) {
        self.inner.restart();
    }

    fn step(&mut self, dt: Duration, events: &[Event], stats: FrameStats) -> bool {
        self.inner.step(dt, events, stats)
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        std::thread::sleep(self.delay);
        self.inner.render(req)
    }

    fn relight(&mut self, lit: Built) {
        self.inner.relight(lit);
    }

    fn apply_voxels(&mut self, change: VoxelChange) {
        self.inner.apply_voxels(change);
    }

    fn set_surface(&mut self, window: Option<Surface>) {
        self.inner.set_surface(window);
    }

    fn surface_lost(&mut self) -> bool {
        self.inner.surface_lost()
    }
}
