//! Copy and Stamp: a ball of a volume captured as samples and pasted elsewhere.
//! Samples are copied whole, never resampled, so a stamp at the same cell size
//! is exact.

use mscent_math::Vec3d;

use super::context::Context;
use super::span::{Span, collect_edited, edit_samples};
use super::volume::{CellSize, Volume};

/// Most cells from the centre to the edge of a clip, which bounds its memory
/// (97 cubed samples) and the time a capture takes.
pub const MAX_CLIP_HALF: i32 = 48;
/// Marks a sample outside the captured ball: a stamp leaves it alone. Real
/// samples are within `-MAX_SAMPLE..=MAX_SAMPLE`.
const OUTSIDE: i8 = i8::MIN;

/// Samples captured from a volume: every one in a ball, as `edited` values and
/// materials.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    cell: CellSize,
    half: i32,
    values: Vec<(i8, u8)>,
}

impl Clip {
    pub fn cell(&self) -> CellSize {
        self.cell
    }

    /// Distance in world units from the centre to the edge of the box around
    /// the ball.
    pub fn reach(&self) -> f64 {
        f64::from(self.half) * self.cell.units()
    }

    fn side(&self) -> i32 {
        2 * self.half + 1
    }

    /// The captured sample at `offset` from the centre sample, if it is in the ball.
    fn at(&self, offset: [i32; 3]) -> Option<(i8, u8)> {
        let side = self.side();
        let [x, y, z] = offset.map(|o| o + self.half);
        let inside = [x, y, z].iter().all(|&c| (0..side).contains(&c));
        let i = usize::try_from(x + side * (y + side * z)).ok()?;
        inside
            .then(|| self.values.get(i).copied())
            .flatten()
            .filter(|&(v, _)| v != OUTSIDE)
    }
}

/// The sample nearest `p`, in the volume's own numbering.
fn nearest(volume: &Volume, p: Vec3d) -> Option<[i32; 3]> {
    let cell = volume.lattice.cell.units();
    let origin = volume.lattice.origin;
    let at = |v: f64, o: i32| {
        let g = (v / cell).round() - f64::from(o);
        (g.abs() < 1.0e9).then_some(g as i32)
    };
    Some([
        at(p.x, origin[0])?,
        at(p.y, origin[1])?,
        at(p.z, origin[2])?,
    ])
}

/// Captures the ball of `radius` world units around `centre`. `None` when the
/// context belongs to another volume or `centre` is out of range. A ball past
/// [`MAX_CLIP_HALF`] cells is cut down to it.
pub fn capture(volume: &Volume, ctx: &mut Context, centre: Vec3d, radius: f64) -> Option<Clip> {
    if ctx.lattice != volume.lattice || !radius.is_finite() {
        return None;
    }
    let cell = volume.lattice.cell.units();
    let half = ((radius / cell).ceil() as i32).clamp(1, MAX_CLIP_HALF);
    let c = nearest(volume, centre)?;
    let span = Span {
        first: c.map(|g| g - half),
        end: c.map(|g| g + half + 1),
    };
    let mut values = Vec::new();
    collect_edited(volume, ctx, span, &mut values);
    let cells = f64::from(half);
    let mut it = values.iter_mut();
    for z in -half..=half {
        for y in -half..=half {
            for x in -half..=half {
                let dist = f64::from(x * x + y * y + z * z).sqrt();
                if let Some(v) = it.next()
                    && dist > cells
                {
                    *v = (OUTSIDE, 0);
                }
            }
        }
    }
    Some(Clip {
        cell: volume.lattice.cell,
        half,
        values,
    })
}

/// Pastes `clip` with its centre at the sample nearest `at`, replacing what is
/// there inside the ball. A clip from another cell size, or a context for
/// another volume, pastes nothing. Returns the chunks to remesh.
pub fn stamp(volume: &mut Volume, ctx: &mut Context, clip: &Clip, at: Vec3d) -> Vec<[u16; 3]> {
    if ctx.lattice != volume.lattice || clip.cell != volume.lattice.cell {
        return Vec::new();
    }
    let Some(c) = nearest(volume, at) else {
        return Vec::new();
    };
    let samples = volume.lattice.samples();
    let first = [0, 1, 2].map(|a| (c[a] - clip.half).max(0));
    let end = [0, 1, 2].map(|a| (c[a] + clip.half + 1).min(samples[a]));
    if (0..3).any(|a| first[a] >= end[a]) {
        return Vec::new();
    }
    let span = Span { first, end };
    edit_samples(volume, ctx, span, &mut |g, _, value, material| {
        let offset = [0, 1, 2].map(|a| g[a] - c[a]);
        clip.at(offset).unwrap_or((value, material))
    })
}
