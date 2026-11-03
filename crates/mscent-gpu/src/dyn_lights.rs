//! The frame's dynamic lights: in `Frame`, for every shader, and for faces a
//! mask a face of the lights that reach it, as `add_lights` in `level/fill.rs`
//! picks them. Voxels take theirs a chunk, actors on the CPU.

use mscent_render::{DynLight, MAX_DYN_LIGHTS};

use crate::device::{Gpu, GpuError};
use crate::faces::storage;

/// `lights` and `light_colors` of `Frame`: a position and radius, then a colour,
/// for each light.
pub(crate) const BYTES: u64 = 32 * MAX_DYN_LIGHTS as u64;
// `Frame` in common.wgsl holds this many: a uniform's arrays cannot take their
// length from an override.
const _: () = assert!(MAX_DYN_LIGHTS == 4);

/// The lights a frame uses, as software cuts them in `level/frame.rs`.
pub(crate) fn used(lights: &[DynLight]) -> &[DynLight] {
    &lights[..lights.len().min(MAX_DYN_LIGHTS)]
}

/// Appends the `Frame` bytes of `lights`. An unused or dark light gets a radius
/// of 0, which reaches nothing.
pub(crate) fn push(bytes: &mut Vec<u8>, lights: &[DynLight]) {
    let at = |i: usize| used(lights).get(i).filter(|l| l.is_lit());
    for i in 0..MAX_DYN_LIGHTS {
        let (pos, radius) = at(i).map_or((Default::default(), 0.0), |l| (l.pos(), l.radius()));
        for value in [pos.x, pos.y, pos.z, radius] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    for i in 0..MAX_DYN_LIGHTS {
        let color = at(i).map_or(Default::default(), DynLight::color);
        for value in [color.x, color.y, color.z, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
}

/// A bit for each lit light of `lights` that `reaches`.
pub(crate) fn mask(lights: &[DynLight], reaches: impl Fn(&DynLight) -> bool) -> u32 {
    let lit = used(lights).iter().enumerate();
    lit.filter(|(_, l)| l.is_lit() && reaches(l))
        .fold(0, |m, (i, _)| m | 1 << i)
}

/// By face, the lights that reach it this frame.
pub(crate) struct FaceMasks {
    pub(crate) buffer: wgpu::Buffer,
    bytes: Vec<u8>,
    /// Whether the buffer holds any bit, so a frame with no lights after one
    /// with none writes nothing.
    any: bool,
}

impl FaceMasks {
    pub(crate) fn new(gpu: &Gpu, faces: usize) -> Result<Self, GpuError> {
        let bytes = vec![0; faces.saturating_mul(4)];
        let buffer = storage(gpu, "face lights", &bytes, 4)?;
        Ok(Self {
            buffer,
            bytes,
            any: false,
        })
    }

    pub(crate) fn write(&mut self, gpu: &Gpu, lights: &[DynLight]) {
        let lit = used(lights).iter().any(DynLight::is_lit);
        if !lit && !self.any {
            return;
        }
        for (face, slot) in self.bytes.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let m = if lit {
                mask(lights, |l| l.reaches_face(face))
            } else {
                0
            };
            slot.copy_from_slice(&m.to_le_bytes());
        }
        self.any = lit;
        gpu.queue.write_buffer(&self.buffer, 0, &self.bytes);
    }
}
