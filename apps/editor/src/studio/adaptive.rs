//! The render thread's internal resolution, scaled to hold the frame budget.

use std::time::Duration;

/// Scales the internal resolution to keep the frame cost near the budget.
pub(super) struct Adaptive {
    pub(super) scale: f32,
}

impl Adaptive {
    pub(super) fn update(&mut self, cost: Duration) {
        let ms = cost.as_secs_f32() * 1000.0;
        if ms > 12.0 {
            self.scale = (self.scale * 0.85).max(0.25);
        } else if ms < 7.0 {
            self.scale = (self.scale * 1.1).min(1.0);
        }
    }

    pub(super) fn size(&self, w: u32, h: u32) -> (u32, u32) {
        let s = |v: u32| ((v as f32 * self.scale) as u32).clamp(16, v.max(16));
        (s(w), s(h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_scale_drops_when_slow_and_recovers_within_bounds() {
        let mut a = Adaptive { scale: 1.0 };
        for _ in 0..30 {
            a.update(Duration::from_millis(100));
        }
        assert_eq!(a.scale, 0.25);
        assert_eq!(a.size(960, 600), (240, 150));
        for _ in 0..60 {
            a.update(Duration::from_millis(2));
        }
        assert_eq!(a.scale, 1.0);
        assert_eq!(a.size(10, 10), (16, 16));
    }
}
