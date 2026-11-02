//! What one entry of the brush list can do, whatever kind it is.

use mscent_math::Vec3d;
use mscent_world::{Brush, BrushError, TexInfo};

use super::document::{BrushDef, Id, Shape};

impl BrushDef {
    /// The axis-aligned bounds `(min, max)`.
    pub fn bounds(&self) -> ([f64; 3], [f64; 3]) {
        match &self.shape {
            Shape::Box { min, max } => (*min, *max),
            Shape::Convex(planes) => match Brush::new(planes, self.op, 0) {
                Ok(b) => {
                    let a = b.bounds();
                    (
                        [a.min.x as f64, a.min.y as f64, a.min.z as f64],
                        [a.max.x as f64, a.max.y as f64, a.max.z as f64],
                    )
                }
                Err(_) => ([0.0; 3], [0.0; 3]),
            },
            Shape::Volume(v) => {
                let (lo, hi) = v.bounds();
                ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z])
            }
        }
    }

    /// Whether the box can be resized.
    pub fn is_box(&self) -> bool {
        matches!(self.shape, Shape::Box { .. })
    }

    /// Moves the brush by `d`. A face whose texture is not locked to the
    /// world keeps its place on the brush (see [`TexInfo::follow_translation`]).
    pub fn translate(&mut self, d: [f64; 3]) {
        let offset = Vec3d::new(d[0], d[1], d[2]);
        for t in &mut self.texinfos {
            t.follow_translation(offset);
        }
        match &mut self.shape {
            Shape::Box { min, max } => {
                for i in 0..3 {
                    min[i] += d[i];
                    max[i] += d[i];
                }
            }
            Shape::Convex(planes) => {
                for p in planes {
                    p.dist += p.normal.x * d[0] + p.normal.y * d[1] + p.normal.z * d[2];
                }
            }
            // A volume's samples sit on its lattice, so it moves by whole cells.
            Shape::Volume(v) => {
                let cell = v.cell().units();
                if let Ok(moved) = v.translated(d.map(|c| (c / cell).round() as i32)) {
                    *v = moved;
                }
            }
        }
    }

    /// Builds the game brush with CSG order `order`.
    pub fn to_brush(&self, order: u32) -> Result<Brush, BrushError> {
        let brush = match &self.shape {
            Shape::Box { min, max } => Brush::cuboid(
                Vec3d::new(min[0], min[1], min[2]),
                Vec3d::new(max[0], max[1], max[2]),
                self.op,
                order,
            )?,
            Shape::Convex(planes) => Brush::new(planes, self.op, order)?,
            // What a volume is to the CSG: its shell, and an add volume has none.
            Shape::Volume(v) => v
                .clone()
                .with_order(order)
                .shell()
                .unwrap_or(Err(BrushError::Empty))?,
        };
        if self.is_volume() {
            return Ok(brush);
        }
        let mut brush = brush.with_material(self.material);
        if self.texinfos.len() == brush.planes().len() {
            for (i, t) in self.texinfos.iter().enumerate() {
                brush.set_texinfo(i, *t);
            }
        }
        Ok(brush)
    }

    /// The mapping of every plane with defaults filled in; empty for an
    /// invalid brush.
    pub fn full_texinfos(&self) -> Vec<TexInfo> {
        self.to_brush(0)
            .map(|b| b.texinfos().to_vec())
            .unwrap_or_default()
    }

    /// Puts `material` on every plane (and makes it the brush's material).
    pub fn set_material(&mut self, material: u16) {
        self.material = material;
        for t in &mut self.texinfos {
            t.material = material;
        }
    }

    pub(super) fn from_brush(id: Id, b: &Brush) -> BrushDef {
        let shape = shape_of(b);
        BrushDef {
            id,
            texinfos: texinfos_for(&shape, b),
            shape,
            op: b.op(),
            material: b.material(),
        }
    }
}

/// The brush's mappings in the plane order [`BrushDef::to_brush`] will
/// build (a box is rebuilt by `Brush::cuboid`, whose plane order may differ
/// from a loaded file's).
fn texinfos_for(shape: &Shape, b: &Brush) -> Vec<TexInfo> {
    let Shape::Box { min, max } = shape else {
        return b.texinfos().to_vec();
    };
    let canon = Brush::cuboid(
        Vec3d::new(min[0], min[1], min[2]),
        Vec3d::new(max[0], max[1], max[2]),
        b.op(),
        0,
    );
    let Ok(canon) = canon else {
        return Vec::new();
    };
    let find = |n: Vec3d| {
        b.planes()
            .iter()
            .position(|p| (p.normal - n).length() < 1e-9)
    };
    let mapped: Option<Vec<TexInfo>> = canon
        .planes()
        .iter()
        .map(|p| find(p.normal).and_then(|i| b.texinfos().get(i).copied()))
        .collect();
    mapped.unwrap_or_default()
}

/// Recognises the six planes of an axis-aligned box.
fn shape_of(b: &Brush) -> Shape {
    let mut min = [f64::NAN; 3];
    let mut max = [f64::NAN; 3];
    if b.planes().len() == 6 {
        for p in b.planes() {
            let n = [p.normal.x, p.normal.y, p.normal.z];
            for (axis, &c) in n.iter().enumerate() {
                let others = n
                    .iter()
                    .enumerate()
                    .all(|(i, o)| i == axis || o.abs() < 1e-9);
                if others && (c - 1.0).abs() < 1e-9 {
                    max[axis] = p.dist;
                } else if others && (c + 1.0).abs() < 1e-9 {
                    min[axis] = -p.dist;
                }
            }
        }
        if min.iter().chain(max.iter()).all(|v| v.is_finite()) {
            return Shape::Box { min, max };
        }
    }
    Shape::Convex(b.planes().to_vec())
}
