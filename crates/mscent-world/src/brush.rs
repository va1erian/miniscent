//! Brushes: convex solids described by outward-facing planes.
//!
//! A [`Brush`] is the intersection of the half-spaces behind its planes
//! (`normal . p <= dist`). It either adds solid ([`Op::Add`]) or carves empty
//! space ([`Op::Subtract`]); brushes are applied in ascending `order`.
//! Every constructor validates, so a `Brush` value is always bounded, convex,
//! non-degenerate and within [`MAX_COORD`].
//!
//! Every plane also carries a [`TexInfo`] (texture mapping and material),
//! parallel to [`Brush::planes`]. [`Brush::with_material`] sets the material
//! of all planes; [`Brush::set_texinfo`] edits one.

use crate::polytope::{self, MAX_COORD, SEED_HALF};
use crate::texinfo::TexInfo;
use mscent_math::{Aabb, DPlane, Polygon, Side, Vec3d, epsilon};

/// Most planes one brush may carry; bounds validation cost on hostile input.
pub const MAX_PLANES: usize = 64;

/// Material id a brush carries until [`Brush::with_material`] sets another.
pub const DEFAULT_MATERIAL: u16 = 0;

/// What a brush does to the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Fills its volume with solid.
    Add,
    /// Carves its volume out as empty space.
    Subtract,
}

/// Why a brush was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrushError {
    /// Fewer than four planes cannot bound a volume.
    TooFewPlanes,
    TooManyPlanes,
    /// A NaN or infinite coordinate, normal or distance.
    NotFinite,
    /// A zero-length normal, collinear defining points, or an inverted box.
    Degenerate,
    /// A plane or vertex lies beyond the allowed coordinate range.
    OutOfRange,
    /// The planes enclose no volume (empty or flat intersection).
    Empty,
    /// The planes leave the region open on some side.
    Unbounded,
    /// A face's vertices lie in front of another face's plane.
    NonConvex,
}

/// A validated convex brush.
#[derive(Clone, Debug, PartialEq)]
pub struct Brush {
    planes: Vec<DPlane>,
    faces: Vec<Polygon>,
    op: Op,
    order: u32,
    /// One per plane, `texinfos[i]` for `planes[i]`.
    texinfos: Vec<TexInfo>,
}

impl Brush {
    /// Builds a brush from outward-facing planes. Normals are normalised;
    /// planes that do not touch the solid (redundant) are dropped.
    pub fn new(planes: &[DPlane], op: Op, order: u32) -> Result<Self, BrushError> {
        let default = |p: &DPlane| TexInfo::default_for_normal(p.normal);
        let infos: Vec<TexInfo> = planes.iter().map(default).collect();
        Self::new_textured(planes, &infos, op, order)
    }

    /// Like [`Brush::new`], with a [`TexInfo`] per input plane. `texinfos`
    /// must be as long as `planes`; each is repaired with
    /// [`TexInfo::sanitized`]. Texinfos follow their planes when redundant
    /// planes are dropped.
    pub fn new_textured(
        planes: &[DPlane],
        texinfos: &[TexInfo],
        op: Op,
        order: u32,
    ) -> Result<Self, BrushError> {
        if texinfos.len() != planes.len() {
            return Err(BrushError::Degenerate);
        }
        if planes.len() < 4 {
            return Err(BrushError::TooFewPlanes);
        }
        if planes.len() > MAX_PLANES {
            return Err(BrushError::TooManyPlanes);
        }
        let planes = planes
            .iter()
            .map(normalize_plane)
            .collect::<Result<Vec<_>, _>>()?;
        let found = polytope::faces(&planes);
        if found.len() < 4 {
            return Err(BrushError::Empty);
        }
        let faces: Vec<Polygon> = found.iter().map(|(_, p)| p.clone()).collect();
        let reach = polytope::max_abs_coord(&faces);
        if reach > MAX_COORD {
            // Vertices near the seed size mean an open region; merely large
            // ones are simply out of range.
            return Err(if reach > 0.5 * SEED_HALF {
                BrushError::Unbounded
            } else {
                BrushError::OutOfRange
            });
        }
        let texinfos = found
            .iter()
            .map(|&(i, _)| texinfos[i].sanitized(planes[i].normal))
            .collect();
        let planes = found.iter().map(|&(i, _)| planes[i]).collect();
        Ok(Self {
            planes,
            faces,
            op,
            order,
            texinfos,
        })
    }

    /// Axis-aligned box. Corners are snapped to the grid first, so the same
    /// editor input always yields the same planes.
    pub fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32) -> Result<Self, BrushError> {
        if !min.is_finite() || !max.is_finite() {
            return Err(BrushError::NotFinite);
        }
        let (lo, hi) = (
            min.snapped(epsilon::SNAP_GRID),
            max.snapped(epsilon::SNAP_GRID),
        );
        if lo.x >= hi.x || lo.y >= hi.y || lo.z >= hi.z {
            return Err(BrushError::Degenerate);
        }
        let axis = Vec3d::new;
        let planes = [
            DPlane::new(axis(1.0, 0.0, 0.0), hi.x),
            DPlane::new(axis(-1.0, 0.0, 0.0), -lo.x),
            DPlane::new(axis(0.0, 1.0, 0.0), hi.y),
            DPlane::new(axis(0.0, -1.0, 0.0), -lo.y),
            DPlane::new(axis(0.0, 0.0, 1.0), hi.z),
            DPlane::new(axis(0.0, 0.0, -1.0), -lo.z),
        ];
        Self::new(&planes, op, order)
    }

    /// Builds from point triples (each wound counter-clockwise seen from
    /// outside), snapping the points to the grid as the editor does.
    pub fn from_point_triples(
        triples: &[[Vec3d; 3]],
        op: Op,
        order: u32,
    ) -> Result<Self, BrushError> {
        let planes = triples
            .iter()
            .map(|t| DPlane::from_snapped_points(t[0], t[1], t[2], epsilon::SNAP_GRID))
            .collect::<Option<Vec<_>>>()
            .ok_or(BrushError::Degenerate)?;
        Self::new(&planes, op, order)
    }

    /// Builds from a closed set of faces (outward winding), rejecting the
    /// set if any face is not convex or sits in front of another's plane.
    pub fn from_polygons(polys: &[Polygon], op: Op, order: u32) -> Result<Self, BrushError> {
        let mut planes = Vec::with_capacity(polys.len());
        for poly in polys {
            if !is_convex(poly) {
                return Err(BrushError::NonConvex);
            }
            planes.push(poly.plane().ok_or(BrushError::Degenerate)?);
        }
        for (i, plane) in planes.iter().enumerate() {
            let outside = polys.iter().enumerate().any(|(j, p)| {
                j != i
                    && p.vertices()
                        .iter()
                        .any(|&v| plane.classify_point(v) == Side::Front)
            });
            if outside {
                return Err(BrushError::NonConvex);
            }
        }
        Self::new(&planes, op, order)
    }

    /// Outward planes, one per face, in the same order as [`Brush::faces`].
    pub fn planes(&self) -> &[DPlane] {
        &self.planes
    }

    /// Bounding polygons, `faces()[i]` lying on `planes()[i]`.
    pub fn faces(&self) -> &[Polygon] {
        &self.faces
    }

    pub fn op(&self) -> Op {
        self.op
    }

    pub fn order(&self) -> u32 {
        self.order
    }

    /// Material of the first plane: what [`Brush::with_material`] set when
    /// all planes share one. Use [`Brush::texinfos`] for per-plane values.
    pub fn material(&self) -> u16 {
        self.texinfos
            .first()
            .map_or(DEFAULT_MATERIAL, |t| t.material)
    }

    /// Returns the brush with `material` on every plane.
    pub fn with_material(mut self, material: u16) -> Self {
        for t in &mut self.texinfos {
            t.material = material;
        }
        self
    }

    /// Texture mapping per plane, `texinfos()[i]` for `planes()[i]`.
    pub fn texinfos(&self) -> &[TexInfo] {
        &self.texinfos
    }

    /// Replaces the TexInfo of plane `index` (repaired with
    /// [`TexInfo::sanitized`]). `false` if the index is out of range.
    pub fn set_texinfo(&mut self, index: usize, info: TexInfo) -> bool {
        let (Some(slot), Some(plane)) = (self.texinfos.get_mut(index), self.planes.get(index))
        else {
            return false;
        };
        *slot = info.sanitized(plane.normal);
        true
    }

    /// Index of the plane whose outward normal is closest to `dir`, `None`
    /// if `dir` is zero or not finite.
    pub fn plane_facing(&self, dir: Vec3d) -> Option<usize> {
        let dir = dir.normalized()?;
        (0..self.planes.len()).max_by(|&a, &b| {
            let (da, db) = (
                self.planes[a].normal.dot(dir),
                self.planes[b].normal.dot(dir),
            );
            da.total_cmp(&db)
        })
    }

    /// Returns the same brush with the add/subtract flag flipped.
    pub fn flipped_op(&self) -> Self {
        let op = match self.op {
            Op::Add => Op::Subtract,
            Op::Subtract => Op::Add,
        };
        Self { op, ..self.clone() }
    }

    /// True if `p` is inside or on the boundary of the brush.
    pub fn contains(&self, p: Vec3d) -> bool {
        self.planes.iter().all(|pl| pl.signed_distance(p) <= 0.0)
    }

    /// Bounding box, rounded to `f32` (runtime precision).
    pub fn bounds(&self) -> Aabb {
        self.faces
            .iter()
            .flat_map(|f| f.vertices().iter())
            .fold(Aabb::empty(), |b, v| b.including(v.to_f32()))
    }
}

fn normalize_plane(p: &DPlane) -> Result<DPlane, BrushError> {
    if !p.normal.is_finite() || !p.dist.is_finite() {
        return Err(BrushError::NotFinite);
    }
    let len = p.normal.length();
    if !len.is_finite() || len < epsilon::MIN_LENGTH_F64 {
        return Err(BrushError::Degenerate);
    }
    let dist = p.dist / len;
    if dist.abs() > MAX_COORD * 2.0 {
        return Err(BrushError::OutOfRange);
    }
    Ok(DPlane::new(p.normal * (1.0 / len), dist))
}

/// A polygon is convex when every corner turns the way its normal says.
fn is_convex(poly: &Polygon) -> bool {
    let Some(normal) = poly.normal() else {
        return false;
    };
    let v = poly.vertices();
    (0..v.len()).all(|i| {
        let (a, b, c) = (v[i], v[(i + 1) % v.len()], v[(i + 2) % v.len()]);
        (b - a).cross(c - b).dot(normal) >= -epsilon::PLANE_EPSILON_F64
    })
}
