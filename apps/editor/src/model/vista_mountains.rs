//! New Mountains: a miniature room of mountains, built small and shown large.
//! A sealed room beyond the level with a rock floor, a ridged-noise range and
//! one big lamp, and a layer at scale 16 as the next of the level's vista. Its
//! walls and ceiling carry the slot's View material, so the layers behind it (the
//! stars of New Vista) show through them. All of it is one undo step.

use mscent_math::{Vec3, Vec3d};
use mscent_world::entity::{MAX_VISTA_FOG, MAX_VISTA_LAYERS};
use mscent_world::materials::{ROCK, VIEW_SLOTS, view_material};
use mscent_world::vista_scale::parallax_of_scale;
use mscent_world::voxel::{CellSize, MountainRange, mountains};
use mscent_world::{Entity, Op};

use super::command::Command;
use super::document::{BrushDef, EntityDef, Shape};
use super::state::{EditorState, Selection, box_ok};

/// Side of the room, in units: at scale 16 it looks 2048 across.
const ROOM: f64 = 128.0;
/// How thick the floor is.
const FLOOR: f64 = 4.0;
/// Scale the layer is built at.
const SCALE: f32 = 16.0;
/// Space between the room's wall and the range, in units.
const WALL_GAP: f64 = 8.0;
/// Sides of the range (seven chunks of one-unit cells), and its tallest peak.
const RANGE: f64 = 112.0;
const PEAK: f64 = 64.0;
/// Voxel material of the rock.
const VOXEL_ROCK: u8 = 4;

impl EditorState {
    /// The slot and layer index a new mountain layer takes: the first free layer
    /// of the level's vista if it has one, else layer 0 of the next free slot.
    fn mountain_layer(&self) -> Result<(u8, u8), String> {
        let Some(slot) = self.vista_slot() else {
            let slot = self.free_vista_slot();
            return slot.map(|s| (s, 0)).ok_or_else(|| {
                format!("All {VIEW_SLOTS} vista slots have a layer: delete one first")
            });
        };
        // The first free layer, so a gap left by a deleted one is used.
        match self.free_layer_in(slot) {
            Some(layer) => Ok((slot, layer)),
            None => Err(format!(
                "Vista slot {slot} has all {MAX_VISTA_LAYERS} layers: delete one first"
            )),
        }
    }

    /// Adds the mountain room and its layer as one undo step, and selects the
    /// layer. With the slot's layers full or no space beyond the level, nothing
    /// changes and the status says why.
    pub fn new_mountains(&mut self) -> bool {
        let (slot, layer) = match self.mountain_layer() {
            Ok(found) => found,
            Err(why) => {
                self.message = why;
                return false;
            }
        };
        let Some((min, max)) = self.room_beyond(ROOM) else {
            self.message = "No room beyond the level for mountains".into();
            return false;
        };
        let brushes = self.doc.brushes().len();
        let Some(range) = self.range_in(min, brushes as u32 + 2) else {
            self.message = "The mountains do not fit beyond the level".into();
            return false;
        };
        let volume = match mountains(&range) {
            Ok(volume) => volume,
            Err(e) => {
                self.message = format!("Cannot make mountains: {e}");
                return false;
            }
        };
        let view = view_material(slot).unwrap_or(ROCK);
        let ids = [(); 5].map(|()| self.doc.alloc_id());
        let [room_id, floor_id, volume_id, lamp_id, layer_id] = ids;
        let mut room = BrushDef {
            id: room_id,
            shape: Shape::Box { min, max },
            op: Op::Subtract,
            material: view,
            texinfos: Vec::new(),
        };
        room.texinfos = room.full_texinfos();
        let floor_top = min[2] + FLOOR;
        let floor = BrushDef {
            id: floor_id,
            shape: Shape::Box {
                min,
                max: [max[0], max[1], floor_top],
            },
            op: Op::Add,
            material: ROCK,
            texinfos: Vec::new(),
        };
        let range_def = BrushDef {
            id: volume_id,
            shape: Shape::Volume(volume),
            op: Op::Add,
            material: u16::from(VOXEL_ROCK),
            texinfos: Vec::new(),
        };
        let middle = [0, 1, 2].map(|i| (min[i] + max[i]) * 0.5);
        let centre = self.vista_centre();
        let lamp = Entity::Light {
            pos: Vec3::new(middle[0] as f32, middle[1] as f32, (max[2] - 24.0) as f32),
            color: Vec3::new(1.0, 0.95, 0.85),
            radius: 400.0,
            intensity: 1.2,
        };
        let parallax = parallax_of_scale(SCALE);
        let pos = Vec3::new(
            middle[0] as f32,
            middle[1] as f32,
            self.eye_height(floor_top, centre.z, parallax, max[2]),
        );
        let layer_entity = Entity::VistaLayer {
            pos,
            centre,
            slot,
            layer,
            parallax,
            spin: Vec3::new(0.0, 0.0, 0.0),
            pan: [0.0, 0.0],
            fog_start: 0.0,
            fog_end: MAX_VISTA_FOG,
            fog_color: 0,
        };
        let entities = self.doc.entities().len();
        let mut cmds: Vec<Command> = [room, floor, range_def]
            .into_iter()
            .enumerate()
            .map(|(i, brush)| Command::AddBrush {
                index: brushes + i,
                brush,
            })
            .collect();
        for (i, entity) in [(lamp_id, lamp), (layer_id, layer_entity)]
            .into_iter()
            .enumerate()
        {
            let entity = EntityDef {
                id: entity.0,
                entity: entity.1,
            };
            cmds.push(Command::AddEntity {
                index: entities + i,
                entity,
            });
        }
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.selection = Some(Selection::Entity(layer_id));
        self.faces.clear();
        self.message = format!("New mountains in slot {slot} layer {layer} at scale {SCALE}");
        true
    }

    /// The range inside the room whose lowest corner is `room_min`, if its box
    /// is one the editor allows.
    fn range_in(&self, room_min: [f64; 3], order: u32) -> Option<MountainRange> {
        let low = [
            room_min[0] + WALL_GAP,
            room_min[1] + WALL_GAP,
            room_min[2] + FLOOR - 3.0,
        ];
        let high = [low[0] + RANGE, low[1] + RANGE, low[2] + 2.0 + PEAK + 1.0];
        box_ok(low, high).then(|| MountainRange {
            min: Vec3d::new(low[0], low[1], low[2]),
            max: Vec3d::new(high[0], high[1], high[2]),
            cell: CellSize::One,
            peak: PEAK,
            seed: order,
            material: VOXEL_ROCK,
            order,
            clear: None,
            edge: None,
        })
    }

    /// Height of the floor of the room the player starts in: the bottom of the
    /// subtract box that holds the start, or else the level's lowest point.
    fn play_floor(&self) -> f64 {
        let start = self.doc.entities().iter().find_map(|e| match e.entity {
            Entity::PlayerStart { pos, .. } => Some([pos.x, pos.y, pos.z].map(f64::from)),
            _ => None,
        });
        let holds = |b: &BrushDef| {
            let (lo, hi) = b.bounds();
            b.op == Op::Subtract
                && start.is_some_and(|p| (0..3).all(|i| lo[i] <= p[i] && p[i] <= hi[i]))
        };
        let room = self.doc.brushes().iter().find(|b| holds(b));
        room.map_or_else(
            || self.doc.bounds().unwrap_or_default().0[2],
            |b| b.bounds().0[2],
        )
    }

    /// Height of the layer's eye so that the room's floor continues the play
    /// space's lowest floor: a plane at `z` in the play space is at
    /// `pos.z + (z - centre.z) * parallax` in the room. Kept inside the room.
    fn eye_height(&self, floor_top: f64, centre_z: f32, parallax: f32, ceiling: f64) -> f32 {
        let play_floor = self.play_floor();
        let eye = floor_top + (f64::from(centre_z) - play_floor) * f64::from(parallax);
        eye.clamp(floor_top + 0.25, ceiling - 1.0) as f32
    }
}
