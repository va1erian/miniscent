//! Fit from a face's texture space to its lightmap grid.

use mscent_math::Vec3;
use mscent_world::lightmap::FaceProjection;

use crate::raster::Vertex;
use crate::texture::Texture;

/// One fitting sample: texel `(u, v)` paired with luxel `(lx, ly)`.
type Pair = [f32; 4];

/// Affine map from unwrapped texel coordinates to continuous luxel
/// coordinates of one face (luxel `i` covers `i..i + 1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UvToLuxel {
    m: [f32; 6],
    /// Unit vectors, in the tangent frame's xy plane, of the texture's +u
    /// axis and of its up axis (-v, the normal map's +y).
    frame: [f32; 4],
}

impl UvToLuxel {
    /// Fit the map from a face's polygon: each vertex's world position gives
    /// its luxel coordinates through `proj`, each vertex's `uv` (times the
    /// texture size) gives the texel coordinates the rasterizer will see.
    /// `None` if the polygon has no area in uv space or a value is not finite.
    pub fn from_vertices(proj: &FaceProjection, verts: &[Vertex], tex: &Texture) -> Option<Self> {
        let (tw, th) = (tex.width() as f32, tex.height() as f32);
        let pts: Vec<Pair> = verts
            .iter()
            .map(|v| {
                let d = v.pos - proj.origin;
                let lx = (d.dot(proj.tangent) - proj.u_min) / proj.luxel_size;
                let ly = (d.dot(proj.bitangent) - proj.v_min) / proj.luxel_size;
                [v.uv.x * tw, v.uv.y * th, lx, ly]
            })
            .collect();
        let (p0, p1, p2) = widest_triangle(&pts)?;
        let det = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
        let solve = |k: usize| {
            let (d1, d2) = (p1[k] - p0[k], p2[k] - p0[k]);
            let a = (d1 * (p2[1] - p0[1]) - (p1[1] - p0[1]) * d2) / det;
            let b = ((p1[0] - p0[0]) * d2 - d1 * (p2[0] - p0[0])) / det;
            (a, b, p0[k] - a * p0[0] - b * p0[1])
        };
        let (a, b, c) = solve(2);
        let (d, e, f) = solve(3);
        let m = [a, b, c, d, e, f];
        let unit = |x: f32, y: f32| {
            let len = x.hypot(y);
            (len > 0.0).then(|| [x / len, y / len])
        };
        let (eu, ev) = (unit(a, d)?, unit(-b, -e)?);
        let frame = [eu[0], eu[1], ev[0], ev[1]];
        (m.iter().chain(&frame).all(|x| x.is_finite())).then_some(Self { m, frame })
    }

    /// Re-express a unit vector given in the lightmap's tangent frame in the
    /// texture's frame (x along +u, y towards the top of the image), which is
    /// the frame the normal maps are authored in. The two frames differ by a
    /// rotation (and possibly a mirror) that depends on the face orientation.
    pub fn to_texture_frame(&self, v: Vec3) -> Vec3 {
        let f = &self.frame;
        Vec3::new(v.x * f[0] + v.y * f[1], v.x * f[2] + v.y * f[3], v.z)
    }

    /// Side of a luxel in texels, as the square root of the map's area scale;
    /// multiplied by a texel rate it gives the luxel rate.
    pub fn luxels_per_texel(&self) -> f32 {
        let m = &self.m;
        (m[0] * m[4] - m[1] * m[3]).abs().sqrt()
    }

    /// The map as `[a, b, c, d, e, f]`: `lx = a u + b v + c`, `ly = d u + e v + f`.
    pub fn coefficients(&self) -> [f32; 6] {
        self.m
    }

    /// What [`Self::to_texture_frame`] multiplies by.
    pub fn frame(&self) -> [f32; 4] {
        self.frame
    }

    /// Luxel coordinates of texel coordinates `(u, v)`.
    pub fn apply(&self, u: f32, v: f32) -> (f32, f32) {
        let m = &self.m;
        (m[0] * u + m[1] * v + m[2], m[3] * u + m[4] * v + m[5])
    }
}

/// Three points of the largest uv-space area, the best-conditioned basis.
fn widest_triangle(pts: &[Pair]) -> Option<(Pair, Pair, Pair)> {
    let p0 = *pts.first()?;
    let dist = |p: &Pair| (p[0] - p0[0]).powi(2) + (p[1] - p0[1]).powi(2);
    let p1 = *pts.iter().max_by(|a, b| dist(a).total_cmp(&dist(b)))?;
    let area =
        |p: &Pair| ((p1[0] - p0[0]) * (p[1] - p0[1]) - (p1[1] - p0[1]) * (p[0] - p0[0])).abs();
    let p2 = *pts.iter().max_by(|a, b| area(a).total_cmp(&area(b)))?;
    (area(&p2) > 1e-9).then_some((p0, p1, p2))
}
