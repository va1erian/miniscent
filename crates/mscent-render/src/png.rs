//! Minimal PNG decoder: 8-bit gray, RGB and RGBA, non-interlaced.
//!
//! The asset pipeline only produces these formats, so everything else is
//! rejected with a clear error rather than half-supported. Chunk CRCs are not
//! verified; the zlib Adler-32 and exact-size checks catch corruption of the
//! pixel data, and nothing else in the file is trusted beyond bounds checks.

use core::fmt;

use crate::inflate::{InflateError, zlib_decompress};

/// Largest accepted side, to bound memory from a hostile header.
pub const MAX_DIMENSION: u32 = 4096;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Why a PNG was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PngError {
    /// Missing signature or a chunk runs past the end of the file.
    Malformed,
    /// No IHDR, no IDAT or IHDR after data.
    MissingChunk,
    /// Zero or oversize dimensions.
    BadDimensions,
    /// Bit depth, colour type or interlace we do not decode.
    Unsupported,
    /// Pixel data has the wrong size or an invalid filter byte.
    BadData,
    /// The zlib stream failed.
    Inflate(InflateError),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "png: {self:?}")
    }
}

impl std::error::Error for PngError {}

impl From<InflateError> for PngError {
    fn from(e: InflateError) -> Self {
        Self::Inflate(e)
    }
}

/// A decoded image with 1 (gray), 3 (RGB) or 4 (RGBA) interleaved channels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub channels: u8,
    pub data: Vec<u8>,
}

impl Image {
    /// Pixels as `0xAARRGGBB`; gray is replicated, missing alpha is opaque.
    pub fn to_argb(&self) -> Vec<u32> {
        let c = usize::from(self.channels);
        self.data
            .chunks_exact(c)
            .map(|p| {
                let (r, g, b, a) = match *p {
                    [g] => (g, g, g, 255),
                    [r, g, b] => (r, g, b, 255),
                    [r, g, b, a] => (r, g, b, a),
                    _ => (0, 0, 0, 255),
                };
                u32::from_be_bytes([a, r, g, b])
            })
            .collect()
    }

    /// First channel of every pixel (the gray value of a height map).
    pub fn first_channel(&self) -> Vec<u8> {
        let c = usize::from(self.channels);
        self.data.chunks_exact(c).map(|p| p[0]).collect()
    }
}

struct Header {
    width: u32,
    height: u32,
    channels: u8,
}

fn parse_header(d: &[u8]) -> Result<Header, PngError> {
    let d: &[u8; 13] = d.try_into().map_err(|_| PngError::Malformed)?;
    let width = u32::from_be_bytes([d[0], d[1], d[2], d[3]]);
    let height = u32::from_be_bytes([d[4], d[5], d[6], d[7]]);
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(PngError::BadDimensions);
    }
    let channels = match (d[8], d[9]) {
        (8, 0) => 1,
        (8, 2) => 3,
        (8, 6) => 4,
        _ => return Err(PngError::Unsupported),
    };
    if d[10] != 0 || d[11] != 0 || d[12] != 0 {
        return Err(PngError::Unsupported);
    }
    Ok(Header {
        width,
        height,
        channels,
    })
}

/// Split the file into IHDR and concatenated IDAT payload.
fn collect_chunks(file: &[u8]) -> Result<(Header, Vec<u8>), PngError> {
    if file.get(..8) != Some(&SIGNATURE[..]) {
        return Err(PngError::Malformed);
    }
    let mut pos = 8usize;
    let mut header = None;
    let mut idat = Vec::new();
    while let Some(head) = file.get(pos..pos + 8) {
        let len = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let kind = &head[4..8];
        let body_start = pos + 8;
        let body_end = body_start.checked_add(len).ok_or(PngError::Malformed)?;
        let body = file.get(body_start..body_end).ok_or(PngError::Malformed)?;
        match kind {
            b"IHDR" => header = Some(parse_header(body)?),
            b"IDAT" if header.is_some() => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        pos = body_end.checked_add(4).ok_or(PngError::Malformed)?;
    }
    match header {
        Some(h) if !idat.is_empty() => Ok((h, idat)),
        _ => Err(PngError::MissingChunk),
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let (ia, ib, ic) = (i16::from(a), i16::from(b), i16::from(c));
    let p = ia + ib - ic;
    let (pa, pb, pc) = ((p - ia).abs(), (p - ib).abs(), (p - ic).abs());
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

/// Undo the per-row filters. `raw` holds `height` rows of `1 + stride` bytes.
fn unfilter(raw: &[u8], stride: usize, bpp: usize, height: usize) -> Result<Vec<u8>, PngError> {
    let mut out = vec![0u8; stride * height];
    for (y, line) in raw.chunks_exact(stride + 1).enumerate() {
        let (filter, src) = (line[0], &line[1..]);
        let (done, rest) = out.split_at_mut(y * stride);
        let prev = if y == 0 {
            None
        } else {
            Some(&done[(y - 1) * stride..])
        };
        let cur = &mut rest[..stride];
        for x in 0..stride {
            let a = if x >= bpp { cur[x - bpp] } else { 0 };
            let b = prev.map_or(0, |p| p[x]);
            let c = if x >= bpp {
                prev.map_or(0, |p| p[x - bpp])
            } else {
                0
            };
            let pred = match filter {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                4 => paeth(a, b, c),
                _ => return Err(PngError::BadData),
            };
            cur[x] = src[x].wrapping_add(pred);
        }
    }
    Ok(out)
}

/// Decode a PNG file image. Never panics on malformed input.
pub fn decode(file: &[u8]) -> Result<Image, PngError> {
    let (h, idat) = collect_chunks(file)?;
    let bpp = usize::from(h.channels);
    let stride = (h.width as usize)
        .checked_mul(bpp)
        .ok_or(PngError::BadDimensions)?;
    let expected = stride
        .checked_add(1)
        .and_then(|row| row.checked_mul(h.height as usize))
        .ok_or(PngError::BadDimensions)?;
    let raw = zlib_decompress(&idat, expected)?;
    if raw.len() != expected {
        return Err(PngError::BadData);
    }
    let data = unfilter(&raw, stride, bpp, h.height as usize)?;
    Ok(Image {
        width: h.width,
        height: h.height,
        channels: h.channels,
        data,
    })
}

#[cfg(test)]
#[path = "png_tests.rs"]
mod png_tests;
