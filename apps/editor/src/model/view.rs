//! Orthographic view math: which world axes a view shows and the pan/zoom
//! transform between world and pixels. World z is up.

/// The three orthographic projections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    /// Looking down: x right, y up.
    Top,
    /// Looking along +y: x right, z up.
    Front,
    /// Looking along -x: y right, z up.
    Side,
}

impl View {
    pub const ALL: [View; 3] = [View::Top, View::Front, View::Side];

    /// World axes shown horizontally and vertically.
    pub fn axes(self) -> (usize, usize) {
        match self {
            View::Top => (0, 1),
            View::Front => (0, 2),
            View::Side => (1, 2),
        }
    }

    /// The world axis the view looks along.
    pub fn hidden(self) -> usize {
        match self {
            View::Top => 2,
            View::Front => 1,
            View::Side => 0,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            View::Top => "Top (X/Y)",
            View::Front => "Front (X/Z)",
            View::Side => "Side (Y/Z)",
        }
    }
}

/// Pan and zoom of one 2D view. `center` is the world point at the middle of
/// the view and `scale` the pixels per world unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera2D {
    pub center: [f64; 2],
    pub scale: f64,
}

pub const MIN_SCALE: f64 = 0.5;
pub const MAX_SCALE: f64 = 400.0;

impl Default for Camera2D {
    fn default() -> Self {
        Camera2D {
            center: [0.0, 0.0],
            scale: 8.0,
        }
    }
}

impl Camera2D {
    /// Pixel position (y down) of a world point in a view of `size` pixels.
    pub fn to_screen(self, p: [f64; 2], size: (f64, f64)) -> (f64, f64) {
        (
            size.0 * 0.5 + (p[0] - self.center[0]) * self.scale,
            size.1 * 0.5 - (p[1] - self.center[1]) * self.scale,
        )
    }

    /// The world point under pixel `(x, y)`.
    pub fn to_world(self, x: f64, y: f64, size: (f64, f64)) -> [f64; 2] {
        [
            self.center[0] + (x - size.0 * 0.5) / self.scale,
            self.center[1] - (y - size.1 * 0.5) / self.scale,
        ]
    }

    /// Drags the world by `(dx, dy)` pixels.
    pub fn pan_pixels(&mut self, dx: f64, dy: f64) {
        self.center[0] -= dx / self.scale;
        self.center[1] += dy / self.scale;
    }

    /// Zooms by `factor`, keeping the world point under `(x, y)` fixed.
    pub fn zoom_at(&mut self, x: f64, y: f64, size: (f64, f64), factor: f64) {
        let before = self.to_world(x, y, size);
        self.scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let after = self.to_world(x, y, size);
        self.center[0] += before[0] - after[0];
        self.center[1] += before[1] - after[1];
    }

    /// Frames the world rectangle `lo..hi` with a margin.
    pub fn fit(&mut self, lo: [f64; 2], hi: [f64; 2], size: (f64, f64)) {
        self.center = [(lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5];
        let w = (hi[0] - lo[0]).max(1.0);
        let h = (hi[1] - lo[1]).max(1.0);
        let s = (size.0 / w).min(size.1 / h) * 0.9;
        self.scale = if s.is_finite() {
            s.clamp(MIN_SCALE, MAX_SCALE)
        } else {
            8.0
        };
    }
}
