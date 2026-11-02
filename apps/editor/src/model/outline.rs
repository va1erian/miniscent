//! A brush's silhouette in an orthographic view, for drawing and picking.

use std::collections::HashMap;

use mscent_math::DPlane;

use super::document::{BrushDef, Id, Shape};

/// Twice the signed area of the turn `o -> a -> b`.
fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// One half of a monotone-chain hull: the points in `pts` order, keeping
/// only left turns, without the last point (it starts the other half).
fn chain<'a>(pts: impl Iterator<Item = &'a [f64; 2]>) -> Vec<[f64; 2]> {
    let mut out: Vec<[f64; 2]> = Vec::new();
    for &p in pts {
        while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
            out.pop();
        }
        out.push(p);
    }
    out.pop();
    out
}

/// The convex hull of `pts`, counter-clockwise.
fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut out = chain(pts.iter());
    out.extend(chain(pts.iter().rev()));
    out
}

impl BrushDef {
    /// The corners of the brush's silhouette along world axes `(a, b)`,
    /// counter-clockwise. A box gives its rectangle; an invalid brush gives
    /// its (empty) bounds rectangle so something is still drawn.
    pub fn outline(&self, a: usize, b: usize) -> Vec<[f64; 2]> {
        let rect = || {
            let (lo, hi) = self.bounds();
            vec![
                [lo[a], lo[b]],
                [hi[a], lo[b]],
                [hi[a], hi[b]],
                [lo[a], hi[b]],
            ]
        };
        if self.is_box() {
            return rect();
        }
        let Ok(brush) = self.to_brush(0) else {
            return rect();
        };
        let axis = |v: &mscent_math::Vec3d, i: usize| [v.x, v.y, v.z][i];
        let pts: Vec<[f64; 2]> = brush
            .faces()
            .iter()
            .flat_map(|f| f.vertices())
            .map(|v| [axis(v, a), axis(v, b)])
            .collect();
        let h = hull(pts);
        if h.len() < 3 { rect() } else { h }
    }

    /// Whether `p` (in the view's axes `(a, b)`) lies on the brush's
    /// silhouette; this is what a click picks.
    pub fn covers(&self, a: usize, b: usize, p: [f64; 2]) -> bool {
        let o = self.outline(a, b);
        (0..o.len()).all(|i| cross(o[i], o[(i + 1) % o.len()], p) >= 0.0)
    }
}

/// Silhouettes of convex brushes for one view, kept while the brush's
/// planes stay the same so a repaint does not rebuild them.
#[derive(Default)]
pub struct OutlineCache {
    entries: HashMap<Id, (Vec<DPlane>, Vec<[f64; 2]>)>,
}

impl OutlineCache {
    /// The silhouette of the convex brush `br` along `(a, b)`.
    pub fn get(&mut self, br: &BrushDef, a: usize, b: usize, brushes: usize) -> &[[f64; 2]] {
        let Shape::Convex(planes) = &br.shape else {
            return &[];
        };
        // Deleted brushes leave entries behind; start over when they pile up.
        if self.entries.len() > brushes {
            self.entries.clear();
        }
        let stale = self
            .entries
            .get(&br.id)
            .is_none_or(|(old, _)| old != planes);
        if stale {
            self.entries
                .insert(br.id, (planes.clone(), br.outline(a, b)));
        }
        &self.entries[&br.id].1
    }
}
