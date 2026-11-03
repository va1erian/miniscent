//! Which rooms, and so which leaves and faces, a point light can reach: one
//! row of the level's room table, read when the light is placed. A light that
//! moves is placed again, which is a leaf lookup and a row read and floods
//! nothing, so a flash behind a wall does not light it and a moving one costs
//! no more than a still one.

use std::sync::Arc;

use mscent_math::Vec3;
use mscent_world::LeafId;
use mscent_world::LeafKind;
use mscent_world::rooms::{NEVER, STEPS_PER_UNIT};

use super::Scene;
use super::rooms::RoomMap;

/// The rooms a light reaches, with the level's map of faces and leaves to
/// rooms to answer by face and by leaf. The bits keep their storage between
/// lights, so placing another light into one allocates only when the level has
/// more rooms than any it has held.
#[derive(Clone, Debug, Default)]
pub struct LightReach {
    map: Option<Arc<RoomMap>>,
    rooms: Vec<u64>,
}

fn reset(bits: &mut Vec<u64>, count: usize) {
    bits.clear();
    bits.resize(count.div_ceil(64), 0);
}

/// Sets bit `i`. Out-of-range bits are left alone.
fn set(bits: &mut [u64], i: usize) {
    if let Some(word) = bits.get_mut(i / 64) {
        *word |= 1u64 << (i % 64);
    }
}

fn get(bits: &[u64], i: usize) -> bool {
    bits.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
}

impl LightReach {
    /// Whether the light reached room `room`.
    pub fn has_room(&self, room: u32) -> bool {
        get(&self.rooms, room as usize)
    }

    /// Whether the light reached `leaf`'s room.
    pub fn has_leaf(&self, leaf: LeafId) -> bool {
        let room = self.map.as_ref().and_then(|m| m.room_of(leaf));
        room.is_some_and(|r| self.has_room(r))
    }

    /// Whether the light reached any room of set `set` (see [`RoomMap`]).
    pub fn has_set(&self, set: u32) -> bool {
        let rooms = self.map.as_ref().map_or(&[][..], |m| m.set_rooms(set));
        rooms.iter().any(|&r| self.has_room(r))
    }

    /// Whether the light reached a room face `face` is in front of. It does not
    /// ask which way the face points or how far it is: the shading does, and
    /// gives such a face nothing.
    pub fn has_face(&self, face: usize) -> bool {
        self.map
            .as_ref()
            .is_some_and(|m| self.has_set(m.set_of_face(face)))
    }

    /// Forgets everything; the next light starts from nothing.
    pub fn clear(&mut self) {
        self.map = None;
        self.rooms.clear();
    }
}

impl Scene {
    /// Replaces `out` with what a light at `pos` of `radius` reaches: the rooms
    /// the table puts within `radius` of the room `pos` is in. A light that is
    /// not finite, has no positive radius, or sits in solid reaches nothing.
    pub fn light_reach(&self, pos: Vec3, radius: f32, out: &mut LightReach) {
        out.clear();
        if !pos.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let leaf = self.bsp.leaf_at_f32(pos);
        if self.bsp.leaf_kind(leaf) != LeafKind::Empty {
            return;
        }
        let rooms = self.rooms.rooms();
        let Some(row) = self.rooms.room_of(leaf).and_then(|r| rooms.row(r)) else {
            return;
        };
        reset(&mut out.rooms, rooms.count());
        // Rounded down in the table, so equal steps are still short of the radius.
        let within = radius * STEPS_PER_UNIT;
        for (room, &step) in row.iter().enumerate() {
            if step != NEVER && f32::from(step) < within {
                set(&mut out.rooms, room);
            }
        }
        out.map = Some(Arc::clone(&self.rooms));
    }

    /// The rooms of the level and what is in front of which.
    pub fn rooms(&self) -> &RoomMap {
        &self.rooms
    }
}
