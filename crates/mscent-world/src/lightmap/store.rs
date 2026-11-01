//! Content key and byte format of a baked lightmap, so a bake whose inputs
//! have not changed can be reused (in memory or from a file) instead of
//! repeated.
//!
//! The key is a hash of exactly what the baker reads: face polygons and
//! normals, lights, and the config values that change the output. The byte
//! format is little endian: magic, version, key, atlas, projections, luxels.
//! The reader checks every count against the bytes that remain and every
//! number for finiteness, so a damaged or hostile file yields an error and
//! the caller bakes again.

use super::atlas::{Atlas, Rect};
use super::uv::FaceProjection;
use super::{BakeConfig, LightFace, Lightmap, Luxel, PointLight};
use mscent_math::Vec3;

pub const MAGIC: [u8; 4] = *b"MSLT";
/// Bumped whenever the baker's output for the same input changes, which
/// also invalidates old files.
pub const VERSION: u16 = 2;
/// Largest accepted file.
pub const MAX_BYTES: usize = 256 << 20;
const MAX_ATLAS_SIDE: u32 = 1 << 16;
const MAX_FACES: usize = 1 << 20;

/// Why a stored lightmap was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    BadMagic,
    UnsupportedVersion(u16),
    Truncated,
    TooLarge,
    /// The file belongs to different bake inputs.
    KeyMismatch,
    /// A count, size or number is out of range or inconsistent.
    Corrupt,
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a, 64 bit: stable across runs, platforms and Rust versions (unlike
/// `DefaultHasher`).
struct Fnv(u64);

impl Fnv {
    fn bytes(&mut self, b: &[u8]) {
        for &x in b {
            self.0 = (self.0 ^ u64::from(x)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    fn f32(&mut self, v: f32) {
        // Adding zero turns -0.0 into 0.0 so equal values hash equally.
        self.u32((v + 0.0).to_bits());
    }

    fn vec3(&mut self, v: Vec3) {
        self.f32(v.x);
        self.f32(v.y);
        self.f32(v.z);
    }
}

/// Hash of everything [`super::bake`] reads. The worker count is left out:
/// it does not change the result.
pub fn input_key(faces: &[LightFace], lights: &[PointLight], cfg: &BakeConfig) -> u64 {
    let mut h = Fnv(FNV_BASIS);
    h.bytes(&MAGIC);
    h.u32(u32::from(VERSION));
    for v in [
        cfg.luxel_size,
        cfg.normal_bias,
        cfg.light_radius,
        cfg.albedo,
    ] {
        h.f32(v);
    }
    h.f32(cfg.bounce_gain);
    h.vec3(cfg.ambient);
    for v in [cfg.shadow_samples, cfg.atlas_width, cfg.atlas_max_height] {
        h.u32(v);
    }
    h.u32(lights.len() as u32);
    for l in lights {
        h.vec3(l.position);
        h.vec3(l.color);
        h.f32(l.radius);
    }
    h.u32(faces.len() as u32);
    for f in faces {
        h.u32(f.vertices.len() as u32);
        for &v in &f.vertices {
            h.vec3(v);
        }
        h.vec3(f.normal);
    }
    h.0
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], StoreError> {
        let end = self.pos.checked_add(n).ok_or(StoreError::Truncated)?;
        let out = self.data.get(self.pos..end).ok_or(StoreError::Truncated)?;
        self.pos = end;
        Ok(out)
    }

    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], StoreError> {
        self.take(N)?.try_into().map_err(|_| StoreError::Truncated)
    }

    fn u16(&mut self) -> Result<u16, StoreError> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    fn u32(&mut self) -> Result<u32, StoreError> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> Result<u64, StoreError> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    /// A finite float; NaN and infinities are `Corrupt`.
    fn f32(&mut self) -> Result<f32, StoreError> {
        let v = f32::from_le_bytes(self.array()?);
        v.is_finite().then_some(v).ok_or(StoreError::Corrupt)
    }

    fn vec3(&mut self) -> Result<Vec3, StoreError> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
}

fn put_vec3(out: &mut Vec<u8>, v: Vec3) {
    for c in [v.x, v.y, v.z] {
        out.extend_from_slice(&c.to_le_bytes());
    }
}

/// One face record: rectangle, four vectors, three floats.
const FACE_BYTES: usize = 16 + 4 * 12 + 3 * 4;
const LUXEL_BYTES: usize = 24;

impl Lightmap {
    /// Serialise for [`Lightmap::from_bytes`]; `key` is the [`input_key`]
    /// of the bake.
    pub fn to_bytes(&self, key: u64) -> Vec<u8> {
        let atlas = self.atlas();
        let mut out = Vec::with_capacity(
            32 + atlas.rects().len() * FACE_BYTES + self.texel_slice().len() * LUXEL_BYTES,
        );
        out.extend_from_slice(&MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&key.to_le_bytes());
        out.extend_from_slice(&atlas.width().to_le_bytes());
        out.extend_from_slice(&atlas.height().to_le_bytes());
        out.extend_from_slice(&(atlas.rects().len() as u32).to_le_bytes());
        for (r, p) in atlas.rects().iter().zip(self.projections()) {
            for v in [r.x, r.y, r.w, r.h] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in [p.origin, p.tangent, p.bitangent, p.normal] {
                put_vec3(&mut out, v);
            }
            for v in [p.u_min, p.v_min, p.luxel_size] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        for l in self.texel_slice() {
            put_vec3(&mut out, l.color);
            put_vec3(&mut out, l.dir);
        }
        let mut sum = Fnv(FNV_BASIS);
        sum.bytes(&out);
        out.extend_from_slice(&sum.0.to_le_bytes());
        out
    }

    /// Parse bytes written by [`Lightmap::to_bytes`]. `key` must equal the
    /// stored one, so a file from other inputs is `KeyMismatch`.
    pub fn from_bytes(data: &[u8], key: u64) -> Result<Self, StoreError> {
        if data.len() > MAX_BYTES {
            return Err(StoreError::TooLarge);
        }
        // The last eight bytes are a checksum of everything before them, so a
        // flipped bit in the luxel data is caught too.
        let split = data.len().checked_sub(8).ok_or(StoreError::Truncated)?;
        let (body, tail) = data.split_at(split);
        let mut sum = Fnv(FNV_BASIS);
        sum.bytes(body);
        let mut r = Reader { data: body, pos: 0 };
        if r.take(4)? != MAGIC {
            return Err(StoreError::BadMagic);
        }
        let version = r.u16()?;
        if version != VERSION {
            return Err(StoreError::UnsupportedVersion(version));
        }
        if r.u64()? != key {
            return Err(StoreError::KeyMismatch);
        }
        if tail != sum.0.to_le_bytes() {
            return Err(StoreError::Corrupt);
        }
        let (aw, ah) = (r.u32()?, r.u32()?);
        let n = r.u32()? as usize;
        let side_ok = |s: u32| s > 0 && s <= MAX_ATLAS_SIDE;
        if !side_ok(aw) || !side_ok(ah) || n > MAX_FACES || n * FACE_BYTES > r.remaining() {
            return Err(StoreError::Corrupt);
        }
        let (mut rects, mut proj) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for _ in 0..n {
            let rect = Rect {
                x: r.u32()?,
                y: r.u32()?,
                w: r.u32()?,
                h: r.u32()?,
            };
            let (origin, tangent) = (r.vec3()?, r.vec3()?);
            let (bitangent, normal) = (r.vec3()?, r.vec3()?);
            let (u_min, v_min, luxel_size) = (r.f32()?, r.f32()?, r.f32()?);
            if luxel_size <= 0.0 {
                return Err(StoreError::Corrupt);
            }
            proj.push(FaceProjection {
                origin,
                tangent,
                bitangent,
                normal,
                u_min,
                v_min,
                luxel_size,
                width: rect.w,
                height: rect.h,
            });
            rects.push(rect);
        }
        let atlas = Atlas::from_parts(aw, ah, rects).ok_or(StoreError::Corrupt)?;
        let count = (aw as usize)
            .checked_mul(ah as usize)
            .ok_or(StoreError::Corrupt)?;
        if count.checked_mul(LUXEL_BYTES) != Some(r.remaining()) {
            return Err(StoreError::Corrupt);
        }
        let mut texels = Vec::with_capacity(count);
        for _ in 0..count {
            texels.push(Luxel {
                color: r.vec3()?,
                dir: r.vec3()?,
            });
        }
        Ok(Self::from_parts(atlas, proj, texels))
    }
}
