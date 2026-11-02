//! A layer's pan: self-lit faces listed with their texture coordinates moved.

use mscent_world::materials::is_self_lit;

use crate::raster::Vertex;

/// How far a layer's self-lit faces have scrolled, and where the shifted
/// vertices go. The storage belongs to the layer's owner so a frame does not
/// make it again.
pub(super) struct Pan<'a> {
    /// Texels moved along each texture axis since time 0.
    pub(super) texels: [f32; 2],
    pub(super) scratch: &'a mut Vec<Vertex>,
}

impl Pan<'_> {
    /// `poly`, scrolled for a face of this material and texture size. The picture
    /// slides towards +U and +V, which is `uv` minus the offset. A lit face is
    /// returned as it is: its luxel lookup comes from the same coordinates.
    pub(super) fn apply<'s>(
        &'s mut self,
        material: u16,
        poly: &'s [Vertex],
        (width, height): (u32, u32),
    ) -> &'s [Vertex] {
        if !is_self_lit(material) {
            return poly;
        }
        // Whole repeats change nothing, and dropping them keeps `f32` exact.
        let by = |texels: f32, size: u32| (texels / size.max(1) as f32).rem_euclid(1.0);
        let (du, dv) = (by(self.texels[0], width), by(self.texels[1], height));
        self.scratch.clear();
        let moved = poly
            .iter()
            .map(|v| Vertex::new(v.pos, v.uv.x - du, v.uv.y - dv));
        self.scratch.extend(moved);
        self.scratch
    }
}
