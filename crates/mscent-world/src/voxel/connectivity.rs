//! Which faces of a chunk air joins, for flood culling in caves.

use super::block::{Block, block_index};
use super::chunk::sample_index;
use super::{CHUNK_EDGE, CHUNK_SAMPLES};

/// A side of a chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    MinX,
    MaxX,
    MinY,
    MaxY,
    MinZ,
    MaxZ,
}

/// One bit per unordered pair of different faces, 15 in all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AirMask(u16);

impl AirMask {
    /// Air joins no faces.
    pub const NONE: AirMask = AirMask(0);
    /// Air joins every pair of faces.
    pub const ALL: AirMask = AirMask((1 << 15) - 1);

    const fn bit(a: Face, b: Face) -> Option<u16> {
        let (a, b) = (a as u16, b as u16);
        if a == b {
            return None;
        }
        let (lo, hi) = if a < b { (a, b) } else { (b, a) };
        // Pairs counted row by row: 5 with face 0, then 4, 3, 2, 1.
        Some(1 << (lo * (11 - lo) / 2 + hi - lo - 1))
    }

    /// Can a line of sight enter by `a` and leave by `b` through connected air?
    /// False when `a` and `b` are the same face.
    pub const fn joins(self, a: Face, b: Face) -> bool {
        match Self::bit(a, b) {
            Some(bit) => self.0 & bit != 0,
            None => false,
        }
    }

    fn add_group(&mut self, touched: u8) {
        for a in FACES {
            for b in FACES {
                if touched & (1 << a as u8) != 0
                    && touched & (1 << b as u8) != 0
                    && let Some(bit) = Self::bit(a, b)
                {
                    self.0 |= bit;
                }
            }
        }
    }
}

const FACES: [Face; 6] = [
    Face::MinX,
    Face::MaxX,
    Face::MinY,
    Face::MaxY,
    Face::MinZ,
    Face::MaxZ,
];

/// Bit for each chunk face a sample lies on.
fn faces_of(x: usize, y: usize, z: usize) -> u8 {
    let last = CHUNK_EDGE - 1;
    let on = |c: usize, low: Face, high: Face| {
        (if c == 0 { 1u8 << low as u8 } else { 0 })
            | (if c == last { 1u8 << high as u8 } else { 0 })
    };
    on(x, Face::MinX, Face::MaxX) | on(y, Face::MinY, Face::MaxY) | on(z, Face::MinZ, Face::MaxZ)
}

/// Floods each group of connected air samples in the chunk and records the
/// faces it reaches.
pub(super) fn air_mask(block: &Block) -> AirMask {
    let mut seen = [false; CHUNK_SAMPLES];
    let mut mask = AirMask::NONE;
    for z in 0..CHUNK_EDGE {
        for y in 0..CHUNK_EDGE {
            for x in 0..CHUNK_EDGE {
                if !seen[sample_index(x, y, z)] && is_air(block, x, y, z) {
                    mask.add_group(flood_group(block, &mut seen, [x, y, z]));
                }
            }
        }
    }
    mask
}

fn is_air(block: &Block, x: usize, y: usize, z: usize) -> bool {
    block.density[block_index(x as i32, y as i32, z as i32)] < 0
}

/// Marks the air group holding `start` and returns the faces it touches.
fn flood_group(block: &Block, seen: &mut [bool; CHUNK_SAMPLES], start: [usize; 3]) -> u8 {
    let mut stack = vec![start];
    seen[sample_index(start[0], start[1], start[2])] = true;
    let mut touched = 0u8;
    while let Some(s) = stack.pop() {
        touched |= faces_of(s[0], s[1], s[2]);
        for axis in 0..3 {
            let mut lower = s;
            let mut upper = s;
            lower[axis] = s[axis].wrapping_sub(1);
            upper[axis] = s[axis] + 1;
            for n in [lower, upper] {
                if n[axis] >= CHUNK_EDGE {
                    continue;
                }
                let i = sample_index(n[0], n[1], n[2]);
                if !seen[i] && is_air(block, n[0], n[1], n[2]) {
                    seen[i] = true;
                    stack.push(n);
                }
            }
        }
    }
    touched
}
