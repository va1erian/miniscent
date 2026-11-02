//! The editable level: brush and entity definitions with stable ids.
//!
//! Brushes keep their CSG order as their position in the list, so reordering is
//! a list move. Entities are the game's own [`Entity`] values plus an id.
//! Names and mover groups refer to those ids, not to list positions, so
//! reordering and deleting cannot point them at the wrong thing.

use mscent_math::DPlane;
use mscent_world::voxel::Volume;
use mscent_world::{BrushError, Entity, EntityLabel, Level, MoverGroup, Op, TexInfo};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A stable identity that survives reordering, deletion and undo.
pub type Id = u32;

/// The geometry of one brush.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// An axis-aligned box, fully editable.
    Box { min: [f64; 3], max: [f64; 3] },
    /// Any other convex brush (a ramp): it can move but not be resized.
    Convex(Vec<DPlane>),
    /// A voxel volume: earlier brushes are what it starts from, later ones cut it.
    Volume(Volume),
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

/// The name and tags of one entity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelDef {
    pub name: Option<String>,
    pub tags: Vec<String>,
}

/// Brushes that move together, by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupDef {
    pub name: String,
    pub tags: Vec<String>,
    pub brushes: Vec<Id>,
}

fn volume_def(id: Id, volume: &Volume) -> BrushDef {
    BrushDef {
        id,
        shape: Shape::Volume(volume.clone()),
        op: volume.op(),
        material: 0,
        texinfos: Vec::new(),
    }
}

fn same_shape(a: &Shape, b: &Shape) -> bool {
    match (a, b) {
        (Shape::Volume(x), Shape::Volume(y)) => x.same_shape(y),
        _ => a == b,
    }
}

/// The whole editable level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    brushes: Vec<BrushDef>,
    entities: Vec<EntityDef>,
    labels: BTreeMap<Id, LabelDef>,
    groups: Vec<GroupDef>,
    next_id: Id,
    /// Where the level is saved; its script is beside it. `None` until saved.
    level_path: Option<PathBuf>,
}

impl Document {
    pub fn new() -> Document {
        Document::default()
    }

    /// Imports a level (brush order is list order).
    pub fn from_level(level: &Level) -> Document {
        let mut doc = Document::new();
        let mut items: Vec<(u32, usize, bool)> = (level.brushes.iter().enumerate())
            .map(|(i, b)| (b.order(), i, false))
            .chain((level.volumes.iter().enumerate()).map(|(i, v)| (v.order(), i, true)))
            .collect();
        items.sort_by_key(|&(order, _, volume)| (order, volume));
        let mut brush_ids = vec![0; level.brushes.len()];
        for (_, index, volume) in items {
            let id = doc.alloc_id();
            if volume {
                doc.brushes.push(volume_def(id, &level.volumes[index]));
            } else {
                brush_ids[index] = id;
                doc.brushes
                    .push(BrushDef::from_brush(id, &level.brushes[index]));
            }
        }
        for e in &level.entities {
            let id = doc.alloc_id();
            doc.entities.push(EntityDef { id, entity: *e });
        }
        doc.import_labels(level, &brush_ids);
        doc
    }

    /// Turns the level's indices into ids; an index out of range is dropped.
    fn import_labels(&mut self, level: &Level, brush_ids: &[Id]) {
        for label in &level.labels {
            if let Some(e) = self.entities.get(label.entity as usize) {
                let def = LabelDef {
                    name: label.name.clone(),
                    tags: label.tags.clone(),
                };
                self.labels.insert(e.id, def);
            }
        }
        for group in &level.mover_groups {
            let ids = group.brushes.iter();
            self.groups.push(GroupDef {
                name: group.name.clone(),
                tags: group.tags.clone(),
                brushes: ids
                    .filter_map(|&b| brush_ids.get(b as usize).copied())
                    .collect(),
            });
        }
    }

    /// Labels of the entities still in the document, in list order.
    pub fn export_labels(&self) -> Vec<EntityLabel> {
        let labelled = self.entities.iter().enumerate();
        labelled
            .filter_map(|(index, e)| {
                let label = self.labels.get(&e.id)?;
                Some(EntityLabel {
                    entity: index as u32,
                    name: label.name.clone(),
                    tags: label.tags.clone(),
                })
            })
            .collect()
    }

    /// Mover groups as indices. A brush that was deleted or made subtractive
    /// leaves its group, and a group left empty is not exported.
    pub fn export_groups(&self) -> Vec<MoverGroup> {
        // Volumes are not brushes of the level, so they do not count here.
        let index_of = |id: &Id| {
            let index = self.brush_index(*id)?;
            let def = &self.brushes[index];
            let before = self.brushes[..index].iter().filter(|b| !b.is_volume());
            (def.op == Op::Add && !def.is_volume()).then_some(before.count() as u32)
        };
        self.groups
            .iter()
            .filter_map(|g| {
                let brushes: Vec<u32> = g.brushes.iter().filter_map(index_of).collect();
                (!brushes.is_empty()).then(|| MoverGroup {
                    name: g.name.clone(),
                    tags: g.tags.clone(),
                    brushes,
                })
            })
            .collect()
    }

    /// Exports to the game's level type; fails on the first invalid brush.
    pub fn to_level(&self) -> Result<Level, BrushError> {
        let (mut brushes, mut volumes) = (Vec::new(), Vec::new());
        for (order, b) in (0u32..).zip(&self.brushes) {
            match b.volume() {
                Some(v) => volumes.push(v.clone().with_order(order)),
                None => brushes.push(b.to_brush(order)?),
            }
        }
        Ok(Level {
            brushes,
            entities: self.entities.iter().map(|e| e.entity).collect(),
            volumes,
            labels: self.export_labels(),
            mover_groups: self.export_groups(),
        })
    }

    /// Whether `other` has the same shapes, operations, entities and volumes, in
    /// the same order; only materials and texture mappings may differ. The
    /// lightmap depends on nothing else. A mover group counts, because it takes
    /// its brushes out of the CSG.
    pub fn same_geometry(&self, other: &Document) -> bool {
        self.entities == other.entities
            && self.groups == other.groups
            && self.brushes.len() == other.brushes.len()
            && self
                .brushes
                .iter()
                .zip(&other.brushes)
                .all(|(a, b)| a.id == b.id && same_shape(&a.shape, &b.shape) && a.op == b.op)
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

    pub fn label(&self, id: Id) -> Option<&LabelDef> {
        self.labels.get(&id)
    }

    pub(super) fn set_label(&mut self, id: Id, label: Option<LabelDef>) {
        match label {
            Some(label) => {
                self.labels.insert(id, label);
            }
            None => {
                self.labels.remove(&id);
            }
        }
    }

    pub(super) fn set_groups(&mut self, groups: Vec<GroupDef>) {
        self.groups = groups;
    }

    /// The mover group a brush belongs to, if any.
    pub fn group_of(&self, brush: Id) -> Option<&GroupDef> {
        self.groups.iter().find(|g| g.brushes.contains(&brush))
    }

    pub(crate) fn name_taken(&self, name: &str) -> bool {
        self.groups.iter().any(|g| g.name == name)
            || self
                .labels
                .values()
                .any(|l| l.name.as_deref() == Some(name))
    }

    pub fn level_path(&self) -> Option<&Path> {
        self.level_path.as_deref()
    }

    pub fn set_level_path(&mut self, path: Option<PathBuf>) {
        self.level_path = path;
    }

    pub fn groups(&self) -> &[GroupDef] {
        &self.groups
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
