//! Little-endian primitives for the level format: a bounds-checked cursor
//! and matching writers. Every read returns `Err` rather than panicking.

use crate::format::FormatError;
use crate::texinfo::TexInfo;
use mscent_math::Vec3;

/// Forward-only reader over a byte slice.
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Takes the next `n` bytes; `n` comes from the file, so it is checked.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        let end = self.pos.checked_add(n).ok_or(FormatError::Truncated)?;
        let out = self.data.get(self.pos..end).ok_or(FormatError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], FormatError> {
        self.take(N)?.try_into().map_err(|_| FormatError::Truncated)
    }

    pub fn u8(&mut self) -> Result<u8, FormatError> {
        Ok(self.array::<1>()?[0])
    }

    pub fn u16(&mut self) -> Result<u16, FormatError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, FormatError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn f32(&mut self) -> Result<f32, FormatError> {
        Ok(f32::from_le_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, FormatError> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    pub fn vec3(&mut self) -> Result<Vec3, FormatError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }

    /// A [`TexInfo`] record ([`TEXINFO_BYTES`]); out-of-range values are an
    /// error, not clamped, so a bad file is reported.
    pub fn texinfo(&mut self) -> Result<TexInfo, FormatError> {
        let material = self.u16()?;
        let (u_axis, v_axis) = (self.vec3()?, self.vec3()?);
        let [u_shift, v_shift, u_scale, v_scale, rotation_deg] = [(); 5].map(|()| self.f32());
        let lock_to_world = match self.u8()? {
            0 => false,
            1 => true,
            _ => return Err(FormatError::BadTexInfo),
        };
        let info = TexInfo {
            material,
            u_axis,
            v_axis,
            u_shift: u_shift?,
            v_shift: v_shift?,
            u_scale: u_scale?,
            v_scale: v_scale?,
            rotation_deg: rotation_deg?,
            lock_to_world,
        };
        info.is_valid()
            .then_some(info)
            .ok_or(FormatError::BadTexInfo)
    }
}

pub fn put_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_f32(out: &mut Vec<u8>, v: f32) {
    out.extend_from_slice(&v.to_le_bytes());
}

pub fn put_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

/// Size of one serialised [`TexInfo`].
pub const TEXINFO_BYTES: usize = 2 + 24 + 20 + 1;

pub fn put_texinfo(out: &mut Vec<u8>, t: &TexInfo) {
    put_u16(out, t.material);
    put_vec3(out, t.u_axis);
    put_vec3(out, t.v_axis);
    for v in [t.u_shift, t.v_shift, t.u_scale, t.v_scale, t.rotation_deg] {
        put_f32(out, v);
    }
    out.push(u8::from(t.lock_to_world));
}

pub fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    put_f32(out, v.x);
    put_f32(out, v.y);
    put_f32(out, v.z);
}

/// A length or count as `u32`; saturates so the reader rejects (rather than
/// silently wraps) an impossible value.
pub fn len_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
