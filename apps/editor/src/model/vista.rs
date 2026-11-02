//! New Vista: the first recipe of `docs/viewports.md` as one undo step. A
//! sealed room of stars beyond the level, a planet on a plate against one wall,
//! and a layer in the room. The mapper then paints faces of the play space with
//! a View material and has a sky.

use mscent_math::{Vec3, Vec3d};
use mscent_world::entity::MAX_VISTA_FOG;
use mscent_world::materials::{SKY_PLANET, SKY_STARS, VIEW_SLOTS};
use mscent_world::{Entity, Op, TexInfo};

use super::command::Command;
use super::document::{BrushDef, EntityDef, Shape};
use super::facetex::DEFAULT_TEXTURE;
use super::state::{EditorState, Selection, box_ok};

/// Side of the room, in units. Walls 64 across with a starfield of 64-unit
/// tiles keep the stars sharp from the middle.
const ROOM: f64 = 64.0;
/// Solid left between the room and the rest of the level. The vista must stay
/// sealed, and 8 units is the least that holds.
const MARGIN: f64 = 32.0;
/// World units per texel of the star wall: a 512 texel tile covers one wall, so
/// a two texel star is about a quarter of a unit and a few pixels from the middle.
const STAR_SCALE: f32 = 0.125;
/// Side and thickness of the plate that carries the planet, and its distance
/// from the wall.
const PLATE: f64 = 24.0;
const PLATE_THICKNESS: f64 = 1.0;
const PLATE_GAP: f64 = 8.0;
/// The planet texture's size if the editor has no table to ask.
const PLANET_TEXTURE: (u32, u32) = (256, 256);
/// A slow turn about the vertical, in radians per second: two minutes a turn.
const SPIN: f32 = 0.05;

fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0] as f32, p[1] as f32, p[2] as f32)
}

impl EditorState {
    /// The slot the next layer should take, or `None` when all are used.
    pub fn free_vista_slot(&self) -> Option<u8> {
        let used = |slot: u8| {
            self.doc
                .entities()
                .iter()
                .any(|e| matches!(e.entity, Entity::VistaLayer { slot: s, .. } if s == slot))
        };
        (0..VIEW_SLOTS).find(|&slot| !used(slot))
    }

    /// The point of the play space a new layer maps to its own position: the
    /// centre of an existing layer, so every layer of a level agrees, or else the
    /// centre of the level.
    pub fn vista_centre(&self) -> Vec3 {
        let existing = self.doc.entities().iter().find_map(|e| match e.entity {
            Entity::VistaLayer { centre, .. } => Some(centre),
            _ => None,
        });
        existing.unwrap_or_else(|| {
            let (lo, hi) = self.doc.bounds().unwrap_or_default();
            vec3([0, 1, 2].map(|i| (lo[i] + hi[i]) * 0.5))
        })
    }

    /// The texel size of a sky material: the table's, or what the stock one is.
    fn sky_size(&self, material: u16) -> (u32, u32) {
        match self.texture_sizes.get(usize::from(material)) {
            Some(&size) => size,
            None if material == SKY_PLANET => PLANET_TEXTURE,
            None => DEFAULT_TEXTURE,
        }
    }

    /// Where a room of `ROOM` units fits beyond the level.
    fn vista_room(&self) -> Option<([f64; 3], [f64; 3])> {
        self.room_beyond(ROOM)
    }

    /// Where a room of `side` units fits beyond the level: east, west, north,
    /// then south of it, always `MARGIN` clear of everything. `None` when the
    /// editor's limits leave no place.
    pub(super) fn room_beyond(&self, side: f64) -> Option<([f64; 3], [f64; 3])> {
        let (lo, hi) = self.doc.bounds().unwrap_or_default();
        let mid = [0, 1, 2].map(|i| ((lo[i] + hi[i]) * 0.5).round());
        let half = side * 0.5;
        let east = [hi[0] + MARGIN, mid[1] - half, mid[2] - half];
        let west = [lo[0] - MARGIN - side, mid[1] - half, mid[2] - half];
        let north = [mid[0] - half, hi[1] + MARGIN, mid[2] - half];
        let south = [mid[0] - half, lo[1] - MARGIN - side, mid[2] - half];
        [east, west, north, south].into_iter().find_map(|min| {
            let min = min.map(f64::round);
            let max = min.map(|v| v + side);
            box_ok(min, max).then_some((min, max))
        })
    }

    /// Adds a sealed sky room beyond the level, a planet plate in it and a layer
    /// for the next free slot, as one undo step, and selects the layer. With
    /// every slot taken, or no room left, nothing changes and the status says why.
    pub fn new_vista(&mut self) -> bool {
        let Some(slot) = self.free_vista_slot() else {
            self.message = format!("All {VIEW_SLOTS} vista slots have a layer: delete one first");
            return false;
        };
        let Some((min, max)) = self.vista_room() else {
            self.message = "No room beyond the level for a vista".into();
            return false;
        };
        let room_id = self.doc.alloc_id();
        let plate_id = self.doc.alloc_id();
        let layer_id = self.doc.alloc_id();
        let centre = self.vista_centre();
        let middle = [0, 1, 2].map(|i| (min[i] + max[i]) * 0.5);
        let room = self.star_room(room_id, min, max);
        let Some(plate) = self.planet_plate(plate_id, min, middle) else {
            self.message = "Cannot make the planet plate".into();
            return false;
        };
        let layer = Entity::VistaLayer {
            pos: vec3(middle),
            centre,
            slot,
            layer: 0,
            parallax: 0.0,
            spin: Vec3::new(0.0, 0.0, SPIN),
            pan: [0.0, 0.0],
            fog_start: 0.0,
            fog_end: MAX_VISTA_FOG,
            fog_color: 0,
        };
        let brushes = self.doc.brushes().len();
        let entities = self.doc.entities().len();
        let cmds = vec![
            Command::AddBrush {
                index: brushes,
                brush: room,
            },
            Command::AddBrush {
                index: brushes + 1,
                brush: plate,
            },
            Command::AddEntity {
                index: entities,
                entity: EntityDef {
                    id: layer_id,
                    entity: layer,
                },
            },
        ];
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.selection = Some(Selection::Entity(layer_id));
        self.faces.clear();
        self.message = format!("New vista in slot {slot}: paint a wall View {slot} to see it");
        true
    }

    /// The room: a subtract box in stars, its texture scaled to show small stars.
    fn star_room(&self, id: u32, min: [f64; 3], max: [f64; 3]) -> BrushDef {
        let mut room = BrushDef {
            id,
            shape: Shape::Box { min, max },
            op: Op::Subtract,
            material: SKY_STARS,
            texinfos: Vec::new(),
        };
        room.texinfos = room.full_texinfos();
        for t in &mut room.texinfos {
            t.set_scale(STAR_SCALE, STAR_SCALE);
        }
        room
    }

    /// A thin add plate near the room's west wall, with its inward face showing
    /// the planet once: the disc fills the face and the rest is clear.
    fn planet_plate(&self, id: u32, room_min: [f64; 3], middle: [f64; 3]) -> Option<BrushDef> {
        let x = room_min[0] + PLATE_GAP;
        let lo = [x, middle[1] - PLATE * 0.5, middle[2] - PLATE * 0.5];
        let hi = [x + PLATE_THICKNESS, lo[1] + PLATE, lo[2] + PLATE];
        let mut plate = BrushDef {
            id,
            shape: Shape::Box { min: lo, max: hi },
            op: Op::Add,
            material: SKY_STARS,
            texinfos: Vec::new(),
        };
        let brush = plate.to_brush(0).ok()?;
        let mut infos = brush.texinfos().to_vec();
        let toward_room = Vec3d::new(1.0, 0.0, 0.0);
        let plane = brush
            .planes()
            .iter()
            .position(|p| (p.normal - toward_room).length() < 1.0e-9)?;
        let info: &mut TexInfo = infos.get_mut(plane)?;
        info.material = SKY_PLANET;
        let face = brush.faces().get(plane)?;
        info.fit(face, self.sky_size(SKY_PLANET), 1.0, 1.0)
            .then_some(())?;
        plate.texinfos = infos;
        Some(plate)
    }
}
