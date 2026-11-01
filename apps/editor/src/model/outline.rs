//! A brush's silhouette in an orthographic view, for drawing.

use super::document::BrushDef;

/// Twice the signed area of the turn `o -> a -> b`.
fn cross(o: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
}

/// The convex hull of `pts`, counter-clockwise (Andrew's monotone chain).
fn hull(mut pts: Vec<[f64; 2]>) -> Vec<[f64; 2]> {
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut out: Vec<[f64; 2]> = Vec::with_capacity(pts.len() * 2);
    for pass in 0..2 {
        let start = out.len();
        let order: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 {
            Box::new(pts.iter())
        } else {
            Box::new(pts.iter().rev())
        };
        for &p in order {
            while out.len() >= start + 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0
            {
                out.pop();
            }
            out.push(p);
        }
        // The last point starts the other chain.
        out.pop();
    }
    out
}

impl BrushDef {
    /// The corners of the brush's silhouette along world axes `(a, b)`, in
    /// order around it. A box gives its rectangle; an invalid brush gives its
    /// (empty) bounds rectangle so something is still drawn.
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
}
