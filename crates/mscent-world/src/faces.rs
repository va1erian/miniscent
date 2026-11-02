//! Visible-surface polygons: the boundary between empty and solid space.
//!
//! [`extract_faces`] walks every empty leaf, builds its convex cell, and for
//! each cell face pushes the polygon back down the tree on the far side. The
//! pieces that land in solid leaves are the visible surface; pieces that land
//! in empty leaves are portals (see the portal stage) and are dropped.
//!
//! Each [`Face`] is wound counter-clockwise seen from the empty side and its
//! plane normal points into empty space. Coplanar neighbours with the same
//! [`TexInfo`] (material and mapping) are merged into larger convex polygons, and finally every edge is
//! subdivided at foreign vertices lying on it so adjacent faces share exactly
//! the same vertices (no T-junctions, hence no pixel cracks).

mod cells;
mod material;
mod merge;
mod tjunction;

use crate::brush::Brush;
use crate::bsp::{Bsp, LeafId};
use crate::texinfo::TexInfo;
use mscent_math::{DPlane, Polygon, epsilon};

/// One planar piece of visible surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Face {
    /// Supporting plane; the normal points into the empty space.
    pub plane: DPlane,
    /// Convex polygon, counter-clockwise seen from the empty side. After
    /// T-junction repair it may carry extra collinear vertices.
    pub polygon: Polygon,
    /// Material id taken from the brush that created the surface; always
    /// equal to `texinfo.material`.
    pub material: u16,
    /// Texture mapping of the brush plane the surface came from. It survives
    /// CSG clipping and merging (faces with different mappings never merge).
    pub texinfo: TexInfo,
    /// Empty leaves in front of the face, sorted and unique. Merging faces
    /// across leaf boundaries unions these lists.
    pub front_leaves: Vec<LeafId>,
    /// Solid leaves behind the face, sorted and unique.
    pub back_leaves: Vec<LeafId>,
}

/// Extracts the visible faces of `bsp`. `brushes` must be the slice the tree
/// was compiled from; they only supply texture mappings, so a mismatch yields
/// default materials, never a panic.
pub fn extract_faces(bsp: &Bsp, brushes: &[Brush]) -> Vec<Face> {
    let mut sorted: Vec<&Brush> = brushes.iter().collect();
    sorted.sort_by_key(|b| b.order());
    let faces = cells::fragments(bsp)
        .into_iter()
        .map(|f| {
            let at = material::centroid(&f.polygon);
            let texinfo = material::texinfo_for(&sorted, &f.plane, at);
            Face {
                material: texinfo.material,
                texinfo,
                plane: f.plane,
                polygon: f.polygon,
                front_leaves: vec![f.empty],
                back_leaves: vec![f.solid],
            }
        })
        .collect();
    let mut faces = merge::merge_coplanar(faces);
    tjunction::fix(&mut faces);
    faces
}

/// True if two oriented planes coincide within the build tolerance.
fn same_plane(a: &DPlane, b: &DPlane) -> bool {
    let eps = epsilon::PLANE_EPSILON_F64;
    (a.normal - b.normal).length() <= eps && (a.dist - b.dist).abs() <= eps
}

/// A mover brush's faces, as authored. They border no leaf: a mover is not part
/// of the world's cells, and the world's faces do not know it is there.
pub fn brush_faces(brush: &crate::brush::Brush) -> Vec<Face> {
    let parts = brush
        .planes()
        .iter()
        .zip(brush.faces())
        .zip(brush.texinfos());
    parts
        .map(|((plane, polygon), texinfo)| Face {
            plane: *plane,
            polygon: polygon.clone(),
            material: texinfo.material,
            texinfo: *texinfo,
            front_leaves: Vec::new(),
            back_leaves: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod faces_tests;
