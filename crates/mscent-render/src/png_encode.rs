//! A minimal PNG encoder for screenshots: 8-bit RGB, or RGBA for a cutout
//! material's picture, stored (uncompressed) deflate blocks, so there is no
//! compression code to get wrong.

const MAX_STORED: usize = 65_535;

fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    for (n, slot) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *slot = c;
    }
    table
}

fn crc32(table: &[u32; 256], chunks: &[&[u8]]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in chunks.iter().copied().flatten() {
        c = table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8);
    }
    !c
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65_521;
        b = (b + a) % 65_521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, table: &[u32; 256], kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(table, &[kind, data]).to_be_bytes());
}

/// Encode `0xAARRGGBB` pixels (alpha ignored) as a PNG file. `None` when the
/// pixel count does not match `width * height` or a side is zero.
pub fn encode_png(width: u32, height: u32, pixels: &[u32]) -> Option<Vec<u8>> {
    encode(width, height, pixels, false)
}

/// [`encode_png`] with the alpha byte kept, as colour type 6.
pub fn encode_png_rgba(width: u32, height: u32, pixels: &[u32]) -> Option<Vec<u8>> {
    encode(width, height, pixels, true)
}

fn encode(width: u32, height: u32, pixels: &[u32], alpha: bool) -> Option<Vec<u8>> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || w.checked_mul(h)? != pixels.len() {
        return None;
    }
    let channels = if alpha { 4 } else { 3 };
    let mut raw = Vec::with_capacity(h * (w * channels + 1));
    for row in pixels.chunks_exact(w) {
        raw.push(0); // filter: none
        for &p in row {
            raw.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
            if alpha {
                raw.push((p >> 24) as u8);
            }
        }
    }
    let mut z = vec![0x78, 0x01];
    let mut blocks = raw.chunks(MAX_STORED).peekable();
    while let Some(block) = blocks.next() {
        z.push(u8::from(blocks.peek().is_none()));
        z.extend_from_slice(&(block.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        z.extend_from_slice(block);
    }
    z.extend_from_slice(&adler32(&raw).to_be_bytes());
    let table = crc_table();
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, if alpha { 6 } else { 2 }, 0, 0, 0]);
    chunk(&mut out, &table, b"IHDR", &ihdr);
    chunk(&mut out, &table, b"IDAT", &z);
    chunk(&mut out, &table, b"IEND", &[]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::png;

    #[test]
    fn round_trips_through_the_decoder() {
        let px: Vec<u32> = (0u32..70 * 40)
            .map(|i| 0xff00_0000 | i.wrapping_mul(2_654_435) & 0xff_ffff)
            .collect();
        let file = encode_png(70, 40, &px).expect("encodes");
        let img = png::decode(&file).expect("decodes");
        assert_eq!((img.width, img.height), (70, 40));
        assert_eq!(img.to_argb(), px);
    }

    #[test]
    fn large_images_use_several_stored_blocks() {
        let px = vec![0xff12_3456; 200 * 200];
        let file = encode_png(200, 200, &px).expect("encodes");
        assert_eq!(png::decode(&file).expect("decodes").to_argb(), px);
    }

    #[test]
    fn rgba_keeps_the_alpha_that_rgb_drops() {
        let px: Vec<u32> = (0u32..30 * 20)
            .map(|i| (i % 256) << 24 | i.wrapping_mul(2_654_435) & 0xff_ffff)
            .collect();
        let file = encode_png_rgba(30, 20, &px).expect("encodes");
        assert_eq!(png::decode(&file).expect("decodes").to_argb(), px);
        let plain = encode_png(30, 20, &px).expect("encodes");
        let opaque: Vec<u32> = px.iter().map(|p| p | 0xff00_0000).collect();
        assert_eq!(png::decode(&plain).expect("decodes").to_argb(), opaque);
    }

    #[test]
    fn rejects_mismatched_sizes() {
        assert!(encode_png(2, 2, &[0; 3]).is_none());
        assert!(encode_png(0, 2, &[]).is_none());
    }
}
