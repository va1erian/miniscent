//! The editor model: document, selection, tool settings and every edit
//! operation. Each operation goes through the undo history.

use mscent_world::{Entity, Op};

use super::command::{Command, History};
use super::document::{BrushDef, Document, EntityDef, Id, Shape};
use super::entity::EntityKind;
use super::pick::FaceRef;
use super::pointer::Drag;

/// What the pointer does in a view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    /// Pick, move and resize.
    Select,
    /// Drag out a new box brush.
    Box,
    /// Click to place an entity.
    Place(EntityKind),
    /// Pick faces in the 3D view and edit their texture mapping.
    Face,
    /// Drag a line across the selected brush to cut it in two.
    Slice,
    /// Drag the selected brush's corners.
    Vertex,
}

/// The one selected item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selection {
    Brush(Id),
    Entity(Id),
}

/// Grid sizes offered by the UI.
pub const GRIDS: [f64; 6] = [0.25, 0.5, 1.0, 2.0, 4.0, 8.0];

/// Largest absolute coordinate the editor accepts.
pub const LIMIT: f64 = 4096.0;

#[derive(Debug)]
pub struct EditorState {
    pub doc: Document,
    pub history: History,
    pub selection: Option<Selection>,
    pub tool: Tool,
    /// Whether newly drawn brushes add or subtract.
    pub new_op: Op,
    pub new_material: u16,
    pub grid: f64,
    /// Extent along the axis a view looks along, for new boxes.
    pub extent: (f64, f64),
    /// Coordinate along the hidden axis for new entities, per world axis.
    pub depth: [f64; 3],
    pub drag: Option<Drag>,
    /// The slice line waiting for Enter.
    pub cut: Option<super::tools::Cut>,
    /// What the next slice keeps.
    pub keep: super::tools::Keep,
    /// The faces picked in the 3D view (see `facetex.rs`).
    pub faces: Vec<FaceRef>,
    pub(super) face_clip: Option<super::facetex::FaceClip>,
    pub(super) face_key: Option<super::facetex::MergeKey>,
    /// Texel size of every material, by id (missing ids count as 128 x 128).
    pub texture_sizes: Vec<(u32, u32)>,
    /// The last thing the user should be told.
    pub message: String,
}

impl Default for EditorState {
    fn default() -> Self {
        EditorState::new(Document::new())
    }
}

impl EditorState {
    pub fn new(doc: Document) -> EditorState {
        EditorState {
            doc,
            history: History::default(),
            selection: None,
            tool: Tool::Select,
            new_op: Op::Subtract,
            new_material: 0,
            grid: 1.0,
            extent: (0.0, 4.0),
            depth: [0.0, 0.0, 1.5],
            drag: None,
            cut: None,
            keep: super::tools::Keep::Both,
            faces: Vec::new(),
            face_clip: None,
            face_key: None,
            texture_sizes: Vec::new(),
            message: "Ready".into(),
        }
    }

    /// Replaces the document and clears history and selection.
    pub fn load(&mut self, doc: Document) {
        self.doc = doc;
        self.history.clear();
        self.selection = None;
        self.faces.clear();
        self.face_clip = None;
        self.face_key = None;
        self.drag = None;
        self.cut = None;
    }

    pub fn snap(&self, v: f64) -> f64 {
        (v / self.grid).round() * self.grid
    }

    pub fn selected_brush(&self) -> Option<&BrushDef> {
        match self.selection {
            Some(Selection::Brush(id)) => self.doc.brush(id),
            _ => None,
        }
    }

    pub fn selected_entity(&self) -> Option<&EntityDef> {
        match self.selection {
            Some(Selection::Entity(id)) => self.doc.entity(id),
            _ => None,
        }
    }

    fn drop_stale_selection(&mut self) {
        let alive = match self.selection {
            Some(Selection::Brush(id)) => self.doc.brush(id).is_some(),
            Some(Selection::Entity(id)) => self.doc.entity(id).is_some(),
            None => true,
        };
        if !alive {
            self.selection = None;
        }
    }

    pub fn undo(&mut self) -> bool {
        self.cancel_drag();
        let ok = self.history.undo(&mut self.doc);
        self.drop_stale_selection();
        self.message = if ok { "Undo" } else { "Nothing to undo" }.into();
        ok
    }

    pub fn redo(&mut self) -> bool {
        self.cancel_drag();
        let ok = self.history.redo(&mut self.doc);
        self.drop_stale_selection();
        self.message = if ok { "Redo" } else { "Nothing to redo" }.into();
        ok
    }

    /// Adds a box brush, selects it and returns its id; `None` when the box
    /// is empty or out of range.
    pub fn add_box(&mut self, min: [f64; 3], max: [f64; 3]) -> Option<Id> {
        if !box_ok(min, max) {
            self.message = "Brush must have a positive size".into();
            return None;
        }
        let id = self.doc.alloc_id();
        let brush = BrushDef {
            id,
            shape: Shape::Box { min, max },
            op: self.new_op,
            material: self.new_material,
            texinfos: Vec::new(),
        };
        let index = self.doc.brushes().len();
        self.history
            .run(&mut self.doc, Command::AddBrush { index, brush });
        self.selection = Some(Selection::Brush(id));
        self.message = format!("Added brush {id}");
        Some(id)
    }

    /// The level's fog `(start, end)`, if it has a world entity.
    pub fn fog(&self) -> Option<(f32, f32)> {
        self.doc.entities().iter().find_map(|e| match e.entity {
            Entity::World {
                fog_start, fog_end, ..
            } => Some((fog_start, fog_end)),
            _ => None,
        })
    }

    /// Sets the level's fog through its world entity, creating the entity if
    /// the level has none; rejected if the range is invalid.
    pub fn set_fog(&mut self, fog_start: f32, fog_end: f32) -> bool {
        let world = Entity::World {
            pos: mscent_math::Vec3::new(0.0, 0.0, 0.0),
            fog_start,
            fog_end,
        };
        if !world.is_valid() {
            self.message = "Fog needs 0 <= start < end <= the maximum depth".into();
            return false;
        }
        let existing = self
            .doc
            .entities()
            .iter()
            .find(|e| matches!(e.entity, Entity::World { .. }))
            .copied();
        let cmd = match existing {
            Some(before) => {
                let mut after = before;
                if let Entity::World {
                    fog_start: s,
                    fog_end: e,
                    ..
                } = &mut after.entity
                {
                    (*s, *e) = (fog_start, fog_end);
                }
                if after == before {
                    return false;
                }
                Command::EditEntity { before, after }
            }
            None => {
                let id = self.doc.alloc_id();
                let index = self.doc.entities().len();
                Command::AddEntity {
                    index,
                    entity: EntityDef { id, entity: world },
                }
            }
        };
        self.history.run(&mut self.doc, cmd);
        self.message = format!("Fog from {fog_start} to {fog_end}");
        true
    }

    /// Places an entity (a second player start replaces the first).
    pub fn place_entity(&mut self, k: EntityKind, p: [f64; 3]) -> Id {
        let id = self.doc.alloc_id();
        let entity = EntityDef {
            id,
            entity: k.make(p),
        };
        let mut cmds = Vec::new();
        // There is one player start.
        if k == EntityKind::PlayerStart {
            for old in self
                .doc
                .entities()
                .iter()
                .filter(|e| matches!(e.entity, Entity::PlayerStart { .. }))
            {
                // Each removal sees the list after the earlier ones.
                let index = self
                    .doc
                    .entity_index(old.id)
                    .unwrap_or(0)
                    .saturating_sub(cmds.len());
                cmds.push(Command::RemoveEntity {
                    index,
                    entity: *old,
                });
            }
        }
        let index = self.doc.entities().len().saturating_sub(cmds.len());
        cmds.push(Command::AddEntity { index, entity });
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.selection = Some(Selection::Entity(id));
        self.message = format!("Placed {}", k.name().to_lowercase());
        id
    }

    pub fn delete_selected(&mut self) -> bool {
        let cmd = match self.selection {
            Some(Selection::Brush(id)) => self.doc.brush_index(id).and_then(|index| {
                let brush = self.doc.brush(id)?.clone();
                Some(Command::RemoveBrush { index, brush })
            }),
            Some(Selection::Entity(id)) => self.doc.entity_index(id).and_then(|index| {
                let entity = *self.doc.entity(id)?;
                Some(Command::RemoveEntity { index, entity })
            }),
            None => None,
        };
        let Some(cmd) = cmd else {
            self.message = "Nothing selected".into();
            return false;
        };
        self.history.run(&mut self.doc, cmd);
        self.selection = None;
        self.faces.clear();
        self.message = "Deleted".into();
        true
    }

    /// Swaps the selected brush between add and subtract.
    pub fn flip_selected(&mut self) -> bool {
        self.edit_selected_brush(|b| {
            b.op = match b.op {
                Op::Add => Op::Subtract,
                Op::Subtract => Op::Add,
            }
        })
    }

    /// Moves the selected brush `delta` places later (positive) or earlier
    /// in the CSG order.
    pub fn reorder_selected(&mut self, delta: i32) -> bool {
        let Some(Selection::Brush(id)) = self.selection else {
            self.message = "Select a brush to reorder".into();
            return false;
        };
        let Some(from) = self.doc.brush_index(id) else {
            return false;
        };
        let last = self.doc.brushes().len() as i64 - 1;
        let to = (from as i64 + delta as i64).clamp(0, last) as usize;
        if to == from {
            return false;
        }
        self.history
            .run(&mut self.doc, Command::Reorder { id, from, to });
        self.message = format!("Brush order {} of {}", to + 1, last + 1);
        true
    }

    /// Applies `f` to the selected brush; rejected if the result is invalid.
    pub fn edit_selected_brush(&mut self, f: impl FnOnce(&mut BrushDef)) -> bool {
        let Some(before) = self.selected_brush().cloned() else {
            return false;
        };
        let mut after = before.clone();
        f(&mut after);
        if let Shape::Box { min, max } = &after.shape
            && !box_ok(*min, *max)
        {
            self.message = "Brush must have a positive size".into();
            return false;
        }
        if after == before {
            return false;
        }
        self.history
            .run(&mut self.doc, Command::EditBrush { before, after });
        true
    }

    /// Applies `f` to the selected entity; rejected if it becomes invalid.
    pub fn edit_selected_entity(&mut self, f: impl FnOnce(&mut Entity)) -> bool {
        let Some(before) = self.selected_entity().copied() else {
            return false;
        };
        let mut after = before;
        f(&mut after.entity);
        if !after.entity.is_valid() {
            self.message = "Entity value out of range".into();
            return false;
        }
        if after == before {
            return false;
        }
        self.history
            .run(&mut self.doc, Command::EditEntity { before, after });
        true
    }

    /// Where a new entity goes for a click at view position `(u, v)`.
    pub fn entity_position(&self, view: super::view::View, p: [f64; 2]) -> [f64; 3] {
        let (a, b) = view.axes();
        let mut out = self.depth;
        out[a] = self.snap(p[0]);
        out[b] = self.snap(p[1]);
        out
    }
}

/// A box must be finite, positive-sized and within [`LIMIT`].
pub fn box_ok(min: [f64; 3], max: [f64; 3]) -> bool {
    (0..3).all(|i| {
        min[i].is_finite()
            && max[i].is_finite()
            && min[i] < max[i]
            && min[i].abs() <= LIMIT
            && max[i].abs() <= LIMIT
    })
}
