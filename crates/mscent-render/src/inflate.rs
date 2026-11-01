//! Minimal zlib/DEFLATE decoder (RFC 1950/1951) for the PNG loader.
//!
//! Written in the style of zlib's `puff.c`: bit-at-a-time canonical Huffman
//! decoding. Textures are small and decoded once at load, so clarity and
//! bounds-safety matter more than speed. Every read is checked and the output
//! size is capped, so a hostile stream cannot over-read or exhaust memory.

use core::fmt;

/// Why a zlib stream was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InflateError {
    /// Input ended before the stream did.
    Truncated,
    /// zlib header is malformed or asks for a preset dictionary.
    BadHeader,
    /// Reserved block type or inconsistent stored block.
    BadBlock,
    /// Huffman table over-subscribed or code not in the table.
    BadCode,
    /// Back-reference reaches before the start of the output.
    BadDistance,
    /// Output would exceed the caller's limit.
    TooLarge,
    /// Adler-32 trailer does not match the data.
    BadChecksum,
}

impl fmt::Display for InflateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid zlib stream: {self:?}")
    }
}

impl std::error::Error for InflateError {}

const LEN_BASE: [u16; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u8; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];
const CLEN_ORDER: [usize; 19] = [
    16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15,
];

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    buf: u32,
    cnt: u32,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            buf: 0,
            cnt: 0,
        }
    }

    /// Read `n` (<= 16) bits, least significant first.
    fn take(&mut self, n: u32) -> Result<u32, InflateError> {
        while self.cnt < n {
            let b = *self.data.get(self.pos).ok_or(InflateError::Truncated)?;
            self.pos += 1;
            self.buf |= u32::from(b) << self.cnt;
            self.cnt += 8;
        }
        let v = self.buf & ((1u32 << n) - 1);
        self.buf >>= n;
        self.cnt -= n;
        Ok(v)
    }

    /// Drop the unread bits of the current byte. Bytes are consumed whole, so
    /// `pos` already points at the next byte boundary.
    fn align_byte(&mut self) {
        self.buf = 0;
        self.cnt = 0;
    }
}

struct Huffman {
    count: [u16; 16],
    symbol: Vec<u16>,
}

impl Huffman {
    /// Build from code lengths. Incomplete codes are allowed (a lone distance
    /// code is legal); over-subscribed ones are not.
    fn new(lengths: &[u8]) -> Result<Self, InflateError> {
        let mut count = [0u16; 16];
        for &l in lengths {
            *count.get_mut(usize::from(l)).ok_or(InflateError::BadCode)? += 1;
        }
        let mut left: i32 = 1;
        for &c in &count[1..] {
            left = (left << 1) - i32::from(c);
            if left < 0 {
                return Err(InflateError::BadCode);
            }
        }
        let mut offs = [0u16; 16];
        for len in 1..15 {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let slot = &mut offs[usize::from(l)];
                symbol[usize::from(*slot)] = sym as u16;
                *slot += 1;
            }
        }
        Ok(Self { count, symbol })
    }

    fn decode(&self, bits: &mut Bits) -> Result<usize, InflateError> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= bits.take(1)? as i32;
            let count = i32::from(self.count[len]);
            if code - count < first {
                let i =
                    usize::try_from(index + (code - first)).map_err(|_| InflateError::BadCode)?;
                return self
                    .symbol
                    .get(i)
                    .map(|&s| usize::from(s))
                    .ok_or(InflateError::BadCode);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err(InflateError::BadCode)
    }
}

fn fixed_tables() -> Result<(Huffman, Huffman), InflateError> {
    let mut l = [8u8; 288];
    l[144..256].fill(9);
    l[256..280].fill(7);
    Ok((Huffman::new(&l)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), InflateError> {
    let nlen = bits.take(5)? as usize + 257;
    let ndist = bits.take(5)? as usize + 1;
    let ncode = bits.take(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError::BadCode);
    }
    let mut cl = [0u8; 19];
    for &slot in CLEN_ORDER.iter().take(ncode) {
        cl[slot] = bits.take(3)? as u8;
    }
    let clh = Huffman::new(&cl)?;
    let mut lengths = [0u8; 320];
    let mut i = 0;
    while i < nlen + ndist {
        let sym = clh.decode(bits)?;
        let (value, repeat) = match sym {
            0..=15 => (sym as u8, 1),
            16 => {
                let prev = *lengths
                    .get(i.wrapping_sub(1))
                    .ok_or(InflateError::BadCode)?;
                (prev, 3 + bits.take(2)? as usize)
            }
            17 => (0, 3 + bits.take(3)? as usize),
            _ => (0, 11 + bits.take(7)? as usize),
        };
        if i + repeat > nlen + ndist {
            return Err(InflateError::BadCode);
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    Ok((
        Huffman::new(&lengths[..nlen])?,
        Huffman::new(&lengths[nlen..nlen + ndist])?,
    ))
}

fn stored_block(bits: &mut Bits, out: &mut Vec<u8>, max: usize) -> Result<(), InflateError> {
    bits.align_byte();
    let hdr = bits
        .data
        .get(bits.pos..bits.pos + 4)
        .ok_or(InflateError::Truncated)?;
    let len = usize::from(u16::from_le_bytes([hdr[0], hdr[1]]));
    let nlen = u16::from_le_bytes([hdr[2], hdr[3]]);
    if nlen != !(len as u16) {
        return Err(InflateError::BadBlock);
    }
    let start = bits.pos + 4;
    let body = bits
        .data
        .get(start..start + len)
        .ok_or(InflateError::Truncated)?;
    if out.len() + len > max {
        return Err(InflateError::TooLarge);
    }
    out.extend_from_slice(body);
    bits.pos = start + len;
    Ok(())
}

fn coded_block(
    bits: &mut Bits,
    out: &mut Vec<u8>,
    max: usize,
    (lit, dist): (&Huffman, &Huffman),
) -> Result<(), InflateError> {
    loop {
        let sym = lit.decode(bits)?;
        if sym < 256 {
            if out.len() >= max {
                return Err(InflateError::TooLarge);
            }
            out.push(sym as u8);
            continue;
        }
        if sym == 256 {
            return Ok(());
        }
        let li = sym - 257;
        let base = *LEN_BASE.get(li).ok_or(InflateError::BadCode)?;
        let len = usize::from(base) + bits.take(u32::from(LEN_EXTRA[li]))? as usize;
        let di = dist.decode(bits)?;
        let dbase = *DIST_BASE.get(di).ok_or(InflateError::BadCode)?;
        let d = usize::from(dbase) + bits.take(u32::from(DIST_EXTRA[di]))? as usize;
        if d > out.len() {
            return Err(InflateError::BadDistance);
        }
        if out.len() + len > max {
            return Err(InflateError::TooLarge);
        }
        // Byte-wise copy: the source may overlap the bytes being written.
        for _ in 0..len {
            out.push(out[out.len() - d]);
        }
    }
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Decode a zlib stream, failing if the result would exceed `max_out` bytes.
pub fn zlib_decompress(data: &[u8], max_out: usize) -> Result<Vec<u8>, InflateError> {
    let (cmf, flg) = match data {
        [c, f, ..] => (*c, *f),
        _ => return Err(InflateError::Truncated),
    };
    let header_ok = cmf & 0x0f == 8 && (u16::from(cmf) * 256 + u16::from(flg)) % 31 == 0;
    if !header_ok || flg & 0x20 != 0 {
        return Err(InflateError::BadHeader);
    }
    let mut bits = Bits::new(&data[2..]);
    let mut out = Vec::new();
    loop {
        let last = bits.take(1)? == 1;
        match bits.take(2)? {
            0 => stored_block(&mut bits, &mut out, max_out)?,
            1 => {
                let (l, d) = fixed_tables()?;
                coded_block(&mut bits, &mut out, max_out, (&l, &d))?;
            }
            2 => {
                let (l, d) = dynamic_tables(&mut bits)?;
                coded_block(&mut bits, &mut out, max_out, (&l, &d))?;
            }
            _ => return Err(InflateError::BadBlock),
        }
        if last {
            break;
        }
    }
    bits.align_byte();
    let tail = bits
        .data
        .get(bits.pos..bits.pos + 4)
        .ok_or(InflateError::Truncated)?;
    if u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]]) != adler32(&out) {
        return Err(InflateError::BadChecksum);
    }
    Ok(out)
}

#[cfg(test)]
#[path = "inflate_tests.rs"]
mod inflate_tests;
