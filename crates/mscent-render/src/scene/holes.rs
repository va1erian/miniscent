//! Viewport faces: walls that show a vista instead of a texture.
//!
//! They stay out of [`Scene::faces`](super::Scene::faces), so they get no luxels
//! and no shader in the per-pixel `match`, and a shot that lands on one finds no
//! surface to mark. In a frame they mark their pixels with the slot, at their true
//! depth so whatever the flood listed beyond the wall stays hidden.
//! [`Framebuffer::open_view`] then turns the marks that survived into the mask a
//! vista draws through.

use mscent_math::{DPlane, Polygon};
use mscent_world::{Face, LeafId};

use super::{Scene, prep};
use crate::framebuffer::{Framebuffer, hole_mark};
use crate::raster::{PolyList, Vertex};
use crate::shade::{SpanEnd, SpanShader};

/// Where the viewport faces of a level are.
#[derive(Clone, Debug, Default)]
pub(super) struct Holes {
    pub(super) polys: Vec<Vec<Vertex>>,
    pub(super) planes: Vec<DPlane>,
    pub(super) slots: Vec<u8>,
    pub(super) leaf_holes: Vec<Vec<u32>>,
}

/// Paints a hole's mark. The texel is ignored, so the faces carry no mapping.
struct MarkShader(u32);

impl SpanShader for MarkShader {
    fn begin_span(&mut self, _start: &SpanEnd, _end: &SpanEnd, _len: u32) {}

    fn shade(&self, _i: u32, _texel: u32, _fu: i32, _fv: i32) -> u32 {
        self.0
    }
}

fn unmapped(polygon: &Polygon) -> Vec<Vertex> {
    polygon
        .vertices()
        .iter()
        .map(|p| Vertex::new(p.to_f32(), 0.0, 0.0))
        .collect()
}

impl Holes {
    /// `faces` paired with their slots.
    pub(super) fn new(leaf_count: usize, faces: &[(Face, u8)]) -> Self {
        let plain: Vec<Face> = faces.iter().map(|(f, _)| f.clone()).collect();
        Self {
            polys: faces.iter().map(|(f, _)| unmapped(&f.polygon)).collect(),
            planes: faces.iter().map(|(f, _)| f.plane).collect(),
            slots: faces.iter().map(|&(_, s)| s).collect(),
            leaf_holes: prep::leaf_faces(leaf_count, &plain),
        }
    }

    /// The viewport faces bordering `leaf`.
    pub(super) fn of_leaf(&self, leaf: LeafId) -> &[u32] {
        self.leaf_holes
            .get(leaf.0 as usize)
            .map_or(&[], Vec::as_slice)
    }
}

impl Scene {
    /// Fills the listed viewport faces in the rows `fb` holds, and returns a bit
    /// for each slot that wrote a pixel. A tag in `list` is a slot.
    pub(crate) fn fill_holes(&self, fb: &mut Framebuffer, list: &PolyList) -> u8 {
        let mut seen = 0;
        for poly in list.in_rows(fb.row_range()) {
            let mut shader = MarkShader(hole_mark(poly.tag));
            if list.fill(fb, poly, &self.fallback, &mut shader).pixels > 0 {
                seen |= 1u8.checked_shl(poly.tag).unwrap_or(0);
            }
        }
        seen
    }
}
