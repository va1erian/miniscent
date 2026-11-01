//! Little-endian primitives for the level format: a bounds-checked cursor
//! and matching writers. Every read returns `Err` rather than panicking.

use crate::format::FormatError;
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
