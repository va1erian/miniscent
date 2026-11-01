//! Adaptive internal resolution and the upscale to the window.
//!
//! The controller is pure logic: feed it the cost of each finished frame and
//! it answers with the scale to render the next one at. Cost grows with the
//! pixel count, so it grows with the square of the scale; that is what lets
//! a drop jump straight to a sensible size and lets recovery predict whether
//! the next step up would blow the budget again.

use crate::framebuffer::Framebuffer;

/// Full resolution, in per-mille of the window size.
pub const FULL: u32 = 1000;

/// Tuning for [`ResolutionController`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolutionConfig {
    /// Frame cost the renderer must stay under, in microseconds.
    pub budget_us: u32,
    /// Smallest scale ever chosen, per-mille of the window.
    pub min_permille: u32,
    /// Granularity of scale changes, per-mille.
    pub step_permille: u32,
    /// Consecutive cheap frames required before scaling back up.
    pub recover_frames: u32,
    /// A step up is taken only if its predicted cost stays under this
    /// percentage of the budget, which prevents drop/recover flapping.
    pub headroom_percent: u32,
}

impl Default for ResolutionConfig {
    fn default() -> Self {
        Self {
            budget_us: 16_600,
            min_permille: 250,
            step_permille: 125,
            recover_frames: 30,
            headroom_percent: 85,
        }
    }
}

/// Chooses the internal render scale from measured frame costs.
#[derive(Clone, Debug)]
pub struct ResolutionController {
    config: ResolutionConfig,
    scale: u32,
    cheap_frames: u32,
}

impl ResolutionController {
    /// Starts at full resolution. `None` for a zero budget or step, a
    /// minimum outside `1..=1000`, or a headroom of 0 or above 100.
    pub fn new(config: ResolutionConfig) -> Option<Self> {
        let ok = config.budget_us > 0
            && config.step_permille > 0
            && (1..=FULL).contains(&config.min_permille)
            && (1..=100).contains(&config.headroom_percent);
        ok.then_some(Self {
            config,
            scale: FULL,
            cheap_frames: 0,
        })
    }

    /// Current scale in per-mille of the window size.
    pub fn scale_permille(&self) -> u32 {
        self.scale
    }

    /// Report the cost of the frame just finished; returns the scale for the
    /// next one.
    pub fn update(&mut self, cost_us: u32) -> u32 {
        if cost_us > self.config.budget_us {
            self.drop_for(cost_us);
        } else if self.can_step_up(cost_us) {
            self.cheap_frames += 1;
            if self.cheap_frames >= self.config.recover_frames {
                self.scale = (self.scale + self.config.step_permille).min(FULL);
                self.cheap_frames = 0;
            }
        } else {
            self.cheap_frames = 0;
        }
        self.scale
    }

    fn drop_for(&mut self, cost_us: u32) {
        self.cheap_frames = 0;
        // Cost scales with area, so the scale that would just fit is the
        // current one times the square root of the budget ratio.
        let ratio = self.config.budget_us as f32 / cost_us as f32;
        let fit = (self.scale as f32 * ratio.sqrt()) as u32;
        // Round down to a step so the scale takes a few distinct values, and
        // always move by at least one step.
        let step = self.config.step_permille;
        let target = fit / step * step;
        let lowered = target.min(self.scale.saturating_sub(step));
        self.scale = lowered.max(self.config.min_permille).min(self.scale);
    }

    /// Whether this frame was cheap enough that one step up would still fit.
    fn can_step_up(&self, cost_us: u32) -> bool {
        if self.scale >= FULL {
            return false;
        }
        let up = (self.scale + self.config.step_permille).min(FULL);
        let predicted = f64::from(cost_us) * f64::from(up).powi(2) / f64::from(self.scale).powi(2);
        let limit = f64::from(self.config.budget_us) * f64::from(self.config.headroom_percent);
        predicted * 100.0 <= limit
    }

    /// Render target size for a window: scaled, at least 1x1, never larger
    /// than the window.
    pub fn internal_size(&self, window_w: u32, window_h: u32) -> (u32, u32) {
        let side = |w: u32| {
            let scaled = u64::from(w) * u64::from(self.scale) / u64::from(FULL);
            (scaled as u32).clamp(1, w.max(1))
        };
        (side(window_w), side(window_h))
    }
}

/// Nearest-neighbour stretch of `src` (`sw` x `sh`) over `dst` (`dw` x `dh`).
/// Integer ratios give exact pixel replication. Returns false, touching
/// nothing, when a size is zero or a slice does not match its dimensions.
pub fn upscale_nearest(src: &[u32], sw: u32, sh: u32, dst: &mut [u32], dw: u32, dh: u32) -> bool {
    let sizes_ok = sw > 0 && sh > 0 && dw > 0 && dh > 0;
    let area = |w: u32, h: u32| (w as usize).checked_mul(h as usize);
    if !sizes_ok || area(sw, sh) != Some(src.len()) || area(dw, dh) != Some(dst.len()) {
        return false;
    }
    let (sw_us, dw_us) = (sw as usize, dw as usize);
    let mut rows = Stepper::new(sh, dh);
    let mut prev_row: Option<usize> = None;
    for y in 0..dh as usize {
        let sy = rows.next_index();
        let (done, rest) = dst.split_at_mut(y * dw_us);
        let out = &mut rest[..dw_us];
        if prev_row == Some(sy) {
            // Same source row as the line above: a plain copy, no sampling.
            out.copy_from_slice(&done[done.len() - dw_us..]);
        } else {
            let line = &src[sy * sw_us..(sy + 1) * sw_us];
            let mut cols = Stepper::new(sw, dw);
            for px in out.iter_mut() {
                *px = line[cols.next_index()];
            }
        }
        prev_row = Some(sy);
    }
    true
}

/// Convenience wrapper for a [`Framebuffer`] source.
pub fn upscale_framebuffer(src: &Framebuffer, dst: &mut [u32], dw: u32, dh: u32) -> bool {
    upscale_nearest(src.pixels(), src.width(), src.height(), dst, dw, dh)
}

/// Maps destination index `i` to source index `floor((2i + 1) * src / (2 dst))`
/// (pixel-centre sampling) without a division per pixel.
struct Stepper {
    index: usize,
    rem: u64,
    step_whole: usize,
    step_rem: u64,
    den: u64,
    first: bool,
}

impl Stepper {
    fn new(src: u32, dst: u32) -> Self {
        let den = 2 * u64::from(dst);
        let start = u64::from(src);
        let step = 2 * u64::from(src);
        Self {
            index: (start / den) as usize,
            rem: start % den,
            step_whole: (step / den) as usize,
            step_rem: step % den,
            den,
            first: true,
        }
    }

    fn next_index(&mut self) -> usize {
        if self.first {
            self.first = false;
            return self.index;
        }
        self.index += self.step_whole;
        self.rem += self.step_rem;
        if self.rem >= self.den {
            self.rem -= self.den;
            self.index += 1;
        }
        self.index
    }
}

#[cfg(test)]
#[path = "resolution_tests.rs"]
mod resolution_tests;
