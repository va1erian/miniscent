//! The level's rooms as the renderer asks about them: a leaf's room, and for a
//! face the set of rooms it is in front of.
//!
//! A face takes the rooms of its front leaves. Almost always that is one room;
//! a face merged across a room's edge is in front of two, and a light that
//! reaches either lights it, which can leak a little light across the thin
//! wall between them. A voxel chunk takes the union of its leaves' rooms the
//! same way (`leaf_chunks`). Faces with the same rooms share a set, so a frame
//! decides each set once and not each face.

use std::collections::HashMap;

use mscent_world::rooms::Rooms;
use mscent_world::{Face, LeafId};

/// Set 0: no room, which a mover's face and a face with no empty leaf in front
/// of it are in.
const NO_ROOMS: u32 = 0;

/// The rooms of a level and the sets of rooms its faces are in front of.
#[derive(Clone, Debug)]
pub struct RoomMap {
    rooms: Rooms,
    /// By face: its set.
    face_set: Vec<u32>,
    /// Set `i` is `set_rooms[set_start[i]..set_start[i + 1]]`.
    set_start: Vec<u32>,
    set_rooms: Vec<u32>,
}

impl RoomMap {
    pub(super) fn new(rooms: Rooms, faces: &[Face]) -> Self {
        let mut sets: HashMap<Vec<u32>, u32> = HashMap::new();
        let mut set_start = vec![0];
        let mut set_rooms = Vec::new();
        let mut intern = |list: Vec<u32>| -> u32 {
            if let Some(&id) = sets.get(&list) {
                return id;
            }
            // Sets are at most the faces, which a `u32` bounds.
            let id = u32::try_from(set_start.len() - 1).unwrap_or(NO_ROOMS);
            set_rooms.extend_from_slice(&list);
            set_start.push(u32::try_from(set_rooms.len()).unwrap_or(u32::MAX));
            sets.insert(list, id);
            id
        };
        intern(Vec::new());
        let face_set = faces
            .iter()
            .map(|f| {
                let mut list: Vec<u32> = f
                    .front_leaves
                    .iter()
                    .filter_map(|&l| rooms.room_of(l))
                    .collect();
                list.sort_unstable();
                list.dedup();
                intern(list)
            })
            .collect();
        Self {
            rooms,
            face_set,
            set_start,
            set_rooms,
        }
    }

    pub fn rooms(&self) -> &Rooms {
        &self.rooms
    }

    /// The room `leaf` is in; `None` for a solid leaf.
    pub fn room_of(&self, leaf: LeafId) -> Option<u32> {
        self.rooms.room_of(leaf)
    }

    /// How many sets there are, the empty one included.
    pub fn set_count(&self) -> usize {
        self.set_start.len() - 1
    }

    /// The set face `face` is in; the empty one for a face out of range.
    pub fn set_of_face(&self, face: usize) -> u32 {
        self.face_set.get(face).copied().unwrap_or(NO_ROOMS)
    }

    /// The rooms of `set`, ascending; none for a set out of range.
    pub fn set_rooms(&self, set: u32) -> &[u32] {
        let at = set as usize;
        let range = self.set_start.get(at).zip(self.set_start.get(at + 1));
        range
            .and_then(|(&a, &b)| self.set_rooms.get(a as usize..b as usize))
            .unwrap_or(&[])
    }
}
