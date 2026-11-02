//! Cave flood: which chunks of a volume a line of sight from the eye can reach
//! through connected air. A straight line only ever moves away from the eye on
//! each axis, so the search never turns back, and it crosses a chunk only if
//! air joins the face it came in by to the face it leaves by. A chunk it reaches
//! is drawn whatever its mask says: the surface it stops at is what you see.

use mscent_math::Vec3;
use mscent_world::voxel::{Face, VolumeGeometry};

/// Faces by axis, low side then high.
const SIDES: [[Face; 2]; 3] = [
    [Face::MinX, Face::MaxX],
    [Face::MinY, Face::MaxY],
    [Face::MinZ, Face::MaxZ],
];

/// Scratch space of a flood, kept so a frame does not allocate it.
#[derive(Default)]
pub(super) struct Flood {
    /// By place in the grid: the faces it has been entered by, bit 6 for none.
    entered: Vec<u8>,
    work: Vec<([i32; 3], Option<Face>)>,
}

const FROM_INSIDE: u8 = 1 << 6;

fn bit(entry: Option<Face>) -> u8 {
    entry.map_or(FROM_INSIDE, |f| 1 << f as u8)
}

fn opposite(f: Face) -> Face {
    match f {
        Face::MinX => Face::MaxX,
        Face::MaxX => Face::MinX,
        Face::MinY => Face::MaxY,
        Face::MaxY => Face::MinY,
        Face::MinZ => Face::MaxZ,
        Face::MaxZ => Face::MinZ,
    }
}

impl Flood {
    /// Floods `volume` from `eye`. Afterwards [`Self::reached`] answers for its
    /// grid.
    pub(super) fn run(&mut self, volume: &VolumeGeometry, eye: Vec3) {
        self.entered.clear();
        self.entered.resize(volume.table.len(), 0);
        self.work.clear();
        let eye_chunk = chunk_of(volume, eye);
        if volume.index(eye_chunk).is_some() {
            self.push(volume, eye_chunk, None);
        } else {
            self.push_sides(volume, eye_chunk);
        }
        while let Some((c, entry)) = self.work.pop() {
            self.cross(volume, c, entry, eye_chunk);
        }
    }

    pub(super) fn reached(&self, index: usize) -> bool {
        self.entered.get(index).is_some_and(|&e| e != 0)
    }

    fn push(&mut self, volume: &VolumeGeometry, c: [i32; 3], entry: Option<Face>) {
        let Some(slot) = volume.index(c).and_then(|i| self.entered.get_mut(i)) else {
            return;
        };
        if *slot & bit(entry) == 0 {
            *slot |= bit(entry);
            self.work.push((c, entry));
        }
    }

    /// From outside the box, a line of sight enters by a side the eye is beyond.
    fn push_sides(&mut self, volume: &VolumeGeometry, eye: [i32; 3]) {
        let size = volume.chunks.map(i32::from);
        for axis in 0..3 {
            let (at, face) = if eye[axis] < 0 {
                (0, SIDES[axis][0])
            } else if eye[axis] >= size[axis] {
                (size[axis] - 1, SIDES[axis][1])
            } else {
                continue;
            };
            let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
            for a in 0..size[u] {
                for b in 0..size[v] {
                    let mut c = [0; 3];
                    c[axis] = at;
                    c[u] = a;
                    c[v] = b;
                    self.push(volume, c, Some(face));
                }
            }
        }
    }

    fn cross(&mut self, volume: &VolumeGeometry, c: [i32; 3], entry: Option<Face>, eye: [i32; 3]) {
        let Some(mask) = volume.index(c).and_then(|i| volume.air.get(i).copied()) else {
            return;
        };
        for axis in 0..3 {
            for (side, step) in [(0, -1), (1, 1)] {
                let away = if step > 0 {
                    c[axis] >= eye[axis]
                } else {
                    c[axis] <= eye[axis]
                };
                let exit = SIDES[axis][side];
                if !away || entry.is_some_and(|e| !mask.joins(e, exit)) {
                    continue;
                }
                let mut next = c;
                next[axis] += step;
                self.push(volume, next, Some(opposite(exit)));
            }
        }
    }
}

/// The chunk holding `p`, which may lie outside the grid.
fn chunk_of(volume: &VolumeGeometry, p: Vec3) -> [i32; 3] {
    let at = |v: f32, o: f32| ((v - o) / volume.chunk_size).floor() as i32;
    [
        at(p.x, volume.origin.x),
        at(p.y, volume.origin.y),
        at(p.z, volume.origin.z),
    ]
}
