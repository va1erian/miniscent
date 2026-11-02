//! One chunk of a volume's stored changes.

use std::sync::Arc;

use super::{CHUNK_EDGE, CHUNK_SAMPLES};

/// Index of a sample inside a chunk. Coordinates must be below [`CHUNK_EDGE`].
pub(super) const fn sample_index(x: usize, y: usize, z: usize) -> usize {
    x + y * CHUNK_EDGE + z * CHUNK_EDGE * CHUNK_EDGE
}

/// The two planes of a chunk that is not uniform.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkData {
    pub(super) delta: [i8; CHUNK_SAMPLES],
    pub(super) material: [u8; CHUNK_SAMPLES],
}

/// A chunk's samples. An edit leaves the uniform form whenever the samples are
/// all alike, so equal contents always compare equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chunk {
    Uniform { delta: i8, material: u8 },
    Detail(Arc<ChunkData>),
}

impl Chunk {
    /// The untouched chunk.
    pub const EMPTY: Chunk = Chunk::Uniform {
        delta: 0,
        material: 0,
    };

    /// `(delta, material)` of sample `i`, which must be in range.
    pub(super) fn sample(&self, i: usize) -> (i8, u8) {
        match self {
            Chunk::Uniform { delta, material } => (*delta, *material),
            Chunk::Detail(d) => (d.delta[i], d.material[i]),
        }
    }

    /// The planes for writing, copied first if an undo step or a snapshot
    /// still shares them.
    pub(super) fn data_mut(&mut self) -> &mut ChunkData {
        if let Chunk::Uniform { delta, material } = *self {
            *self = Chunk::Detail(Arc::new(ChunkData {
                delta: [delta; CHUNK_SAMPLES],
                material: [material; CHUNK_SAMPLES],
            }));
        }
        match self {
            Chunk::Detail(d) => Arc::make_mut(d),
            Chunk::Uniform { .. } => unreachable!("promoted above"),
        }
    }

    /// Returns to the uniform form if the samples allow it.
    pub(super) fn normalize(&mut self) {
        let Chunk::Detail(d) = self else {
            return;
        };
        let (delta, material) = (d.delta[0], d.material[0]);
        if d.delta.iter().all(|&v| v == delta) && d.material.iter().all(|&v| v == material) {
            *self = Chunk::Uniform { delta, material };
        }
    }
}
