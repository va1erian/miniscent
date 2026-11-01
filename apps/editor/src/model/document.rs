//! The editable level: brush and entity definitions with stable ids.
//!
//! Brushes keep their CSG order as their position in the list, so reordering is
//! a list move. Entities are the game's own [`Entity`] values plus an id.

use mscent_math::{DPlane, Vec3d};
use mscent_world::{Brush, BrushError, Entity, Level, Op, TexInfo};

/// A stable identity that survives reordering, deletion and undo.
pub type Id = u32;

/// The geometry of one brush.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned box, fully editable.
    Box { min: [f64; 3], max: [f64; 3] },
    /// Any other convex brush (a ramp): it can move but not be resized.
    Convex(Vec<DPlane>),
}

/// One brush as the editor sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct BrushDef {
    pub id: Id,
    pub shape: Shape,
    pub op: Op,
    pub material: u16,
    /// Texture mapping per brush plane (in the order of
    /// [`BrushDef::to_brush`]), kept so loading and saving a level does not
    /// lose it. Empty means the defaults with `material` on every plane;
    /// otherwise each entry carries its own material.
    pub texinfos: Vec<TexInfo>,
}

/// One placed entity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EntityDef {
    pub id: Id,
    pub entity: Entity,
}

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
        };
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

    fn from_brush(id: Id, b: &Brush) -> BrushDef {
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

/// The whole editable level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    brushes: Vec<BrushDef>,
    entities: Vec<EntityDef>,
    next_id: Id,
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    /// Imports a level (brush order is list order).
    pub fn from_level(level: &Level) -> Document {
        let mut doc = Document::new();
        let mut brushes: Vec<&Brush> = level.brushes.iter().collect();
        brushes.sort_by_key(|b| b.order());
        for b in brushes {
            let id = doc.alloc_id();
            doc.brushes.push(BrushDef::from_brush(id, b));
        }
        for e in &level.entities {
            let id = doc.alloc_id();
            doc.entities.push(EntityDef { id, entity: *e });
        }
        doc
    }

    /// Exports to the game's level type; fails on the first invalid brush.
    pub fn to_level(&self) -> Result<Level, BrushError> {
        let brushes = self
            .brushes
            .iter()
            .enumerate()
            .map(|(i, b)| b.to_brush(i as u32))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Level {
            brushes,
            entities: self.entities.iter().map(|e| e.entity).collect(),
        })
    }

    /// Whether `other` has the same shapes, operations and entities, in the
    /// same order; only materials and texture mappings may differ. The
    /// lightmap depends on nothing else.
    pub fn same_geometry(&self, other: &Document) -> bool {
        self.entities == other.entities
            && self.brushes.len() == other.brushes.len()
            && self
                .brushes
                .iter()
                .zip(&other.brushes)
                .all(|(a, b)| a.id == b.id && a.shape == b.shape && a.op == b.op)
    }

    pub fn alloc_id(&mut self) -> Id {
        self.next_id += 1;
        self.next_id
    }

    pub fn brushes(&self) -> &[BrushDef] {
        &self.brushes
    }

    pub fn entities(&self) -> &[EntityDef] {
        &self.entities
    }

    pub fn brush(&self, id: Id) -> Option<&BrushDef> {
        self.brushes.iter().find(|b| b.id == id)
    }

    pub fn entity(&self, id: Id) -> Option<&EntityDef> {
        self.entities.iter().find(|e| e.id == id)
    }

    pub fn brush_index(&self, id: Id) -> Option<usize> {
        self.brushes.iter().position(|b| b.id == id)
    }

    pub fn entity_index(&self, id: Id) -> Option<usize> {
        self.entities.iter().position(|e| e.id == id)
    }

    pub(super) fn insert_brush(&mut self, index: usize, def: BrushDef) {
        self.next_id = self.next_id.max(def.id);
        self.brushes.insert(index.min(self.brushes.len()), def);
    }

    pub(super) fn remove_brush(&mut self, id: Id) -> Option<(usize, BrushDef)> {
        let i = self.brush_index(id)?;
        Some((i, self.brushes.remove(i)))
    }

    pub(super) fn replace_brush(&mut self, def: &BrushDef) {
        if let Some(slot) = self.brushes.iter_mut().find(|b| b.id == def.id) {
            *slot = def.clone();
        }
    }

    /// Lets tests build a corrupt document that the model itself refuses.
    #[cfg(test)]
    pub fn replace_brush_for_test(&mut self, def: &BrushDef) {
        self.replace_brush(def);
    }

    #[cfg(test)]
    pub fn insert_entity_for_test(&mut self, id: Id, entity: Entity) {
        self.insert_entity(self.entities.len(), EntityDef { id, entity });
    }

    pub(super) fn move_brush(&mut self, id: Id, to: usize) {
        if let Some(from) = self.brush_index(id) {
            let b = self.brushes.remove(from);
            self.brushes.insert(to.min(self.brushes.len()), b);
        }
    }

    pub(super) fn insert_entity(&mut self, index: usize, def: EntityDef) {
        self.next_id = self.next_id.max(def.id);
        self.entities.insert(index.min(self.entities.len()), def);
    }

    pub(super) fn remove_entity(&mut self, id: Id) -> Option<(usize, EntityDef)> {
        let i = self.entity_index(id)?;
        Some((i, self.entities.remove(i)))
    }

    /// Moves an entity the way a drag does, for tests outside the model.
    #[cfg(test)]
    pub fn replace_entity_for_test(&mut self, def: &EntityDef) {
        self.replace_entity(def);
    }
    pub(super) fn replace_entity(&mut self, def: &EntityDef) {
        if let Some(slot) = self.entities.iter_mut().find(|e| e.id == def.id) {
            *slot = *def;
        }
    }

    /// Bounds of every brush and entity, or `None` when empty.
    pub fn bounds(&self) -> Option<([f64; 3], [f64; 3])> {
        let mut acc: Option<([f64; 3], [f64; 3])> = None;
        let mut include = |lo: [f64; 3], hi: [f64; 3]| {
            let (mn, mx) = acc.get_or_insert((lo, hi));
            for i in 0..3 {
                mn[i] = mn[i].min(lo[i]);
                mx[i] = mx[i].max(hi[i]);
            }
        };
        for b in &self.brushes {
            let (lo, hi) = b.bounds();
            include(lo, hi);
        }
        for e in &self.entities {
            let p = super::entity::pos(&e.entity);
            include(p, p);
        }
        acc
    }
}
