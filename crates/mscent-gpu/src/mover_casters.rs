//! What movers add to a light's shadow map: every face of every mover, whichever
//! way the camera sees it, listed once for the level, and each frame the spheres
//! that say where the movers are now so that a light draws only those it meets.

use std::ops::Range;

use mscent_render::{LevelAssets, MoverPose};

use crate::device::{Gpu, GpuError};
use crate::faces::buffer;
use crate::shadows::MoverDraw;

const INDEX_BYTES: usize = 4;

/// A fan of triangles from the first of `corners`, as indices appended to `out`.
pub(crate) fn push_fan(corners: &Range<u32>, out: &mut Vec<u8>) {
    for i in corners.start.saturating_add(1)..corners.end.saturating_sub(1) {
        for index in [corners.start, i, i + 1] {
            out.extend_from_slice(&index.to_le_bytes());
        }
    }
}

pub(crate) struct MoverList {
    pub(crate) indices: wgpu::Buffer,
    /// By mover, its triangles' range of `indices`, counted in indices.
    ranges: Vec<Range<u32>>,
    /// This frame's: the movers with triangles, where they now are.
    pub(crate) draws: Vec<MoverDraw>,
}

impl MoverList {
    /// `corners` is each face's range of the level's vertices, empty for a face
    /// the rasterizer would refuse.
    pub(crate) fn new(
        gpu: &Gpu,
        assets: &LevelAssets,
        corners: &[Range<u32>],
    ) -> Result<Self, GpuError> {
        let mut bytes = Vec::new();
        let mut ranges = Vec::with_capacity(assets.movers().len());
        for mover in assets.movers() {
            let start = bytes.len();
            for face in mover.faces.clone() {
                if let Some(c) = corners.get(face) {
                    push_fan(c, &mut bytes);
                }
            }
            let count = |at: usize| u32::try_from(at / INDEX_BYTES);
            let range = count(start).and_then(|s| Ok(s..count(bytes.len())?));
            ranges.push(range.map_err(|_| GpuError::TooLarge("mover indices"))?);
        }
        let indices = buffer(
            gpu,
            "mover casters",
            (bytes.len() as u64).max(INDEX_BYTES as u64),
            wgpu::BufferUsages::INDEX,
        )?;
        gpu.queue.write_buffer(&indices, 0, &bytes);
        Ok(Self {
            indices,
            ranges,
            draws: Vec::new(),
        })
    }

    /// Where each mover is this frame, in the same order as `poses`' entries in
    /// the pose buffer: mover `i` is pose `i + 1`.
    pub(crate) fn list(&mut self, assets: &LevelAssets, poses: &[MoverPose]) {
        self.draws.clear();
        for (i, (mover, range)) in assets.movers().iter().zip(&self.ranges).enumerate() {
            let pose = poses.get(i).copied().unwrap_or(MoverPose::AUTHORED);
            self.draws.push(MoverDraw {
                pose: i as u32 + 1,
                indices: range.clone(),
                centre: pose.to_world(mover.centre),
                radius: mover.radius,
            });
        }
    }
}
