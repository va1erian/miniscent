use super::*;
use mscent_math::rng::Rng;

fn adler(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    out.extend_from_slice(&[0; 4]); // CRC is not verified by the decoder.
}

/// Build a PNG whose IDAT is one stored zlib block holding `raw`.
fn make_png(w: u32, h: u32, color: u8, raw: &[u8]) -> Vec<u8> {
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend_from_slice(&(raw.len() as u16).to_le_bytes());
    z.extend_from_slice(&(!(raw.len() as u16)).to_le_bytes());
    z.extend_from_slice(raw);
    z.extend_from_slice(&adler(raw).to_be_bytes());
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, color, 0, 0, 0]);
    let mut f = SIGNATURE.to_vec();
    chunk(&mut f, b"IHDR", &ihdr);
    chunk(&mut f, b"IDAT", &z);
    chunk(&mut f, b"IEND", &[]);
    f
}

#[test]
fn all_five_filters_decode() {
    // 2x5 RGB image; each row uses a different filter type 0..=4.
    let rows: [[u8; 6]; 5] = [
        [10, 20, 30, 40, 50, 60],
        [11, 22, 33, 44, 55, 66],
        [100, 90, 80, 70, 60, 50],
        [1, 2, 3, 4, 5, 6],
        [200, 150, 100, 50, 25, 12],
    ];
    let mut raw = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        raw.push(y as u8);
        for x in 0..6 {
            let a = if x >= 3 { row[x - 3] } else { 0 };
            let b = if y > 0 { rows[y - 1][x] } else { 0 };
            let c = if y > 0 && x >= 3 {
                rows[y - 1][x - 3]
            } else {
                0
            };
            let pred = match y {
                0 => 0,
                1 => a,
                2 => b,
                3 => ((u16::from(a) + u16::from(b)) / 2) as u8,
                _ => paeth(a, b, c),
            };
            raw.push(row[x].wrapping_sub(pred));
        }
    }
    let img = decode(&make_png(2, 5, 2, &raw)).unwrap();
    assert_eq!((img.width, img.height, img.channels), (2, 5, 3));
    let expected: Vec<u8> = rows.iter().flatten().copied().collect();
    assert_eq!(img.data, expected);
    assert_eq!(img.to_argb()[0], 0xff0a_141e);
}

#[test]
fn rgba_and_gray_conversion() {
    let img = decode(&make_png(1, 1, 6, &[0, 1, 2, 3, 4])).unwrap();
    assert_eq!(img.to_argb(), vec![0x0401_0203]);
    let g = decode(&make_png(2, 1, 0, &[0, 7, 9])).unwrap();
    assert_eq!(g.to_argb(), vec![0xff07_0707, 0xff09_0909]);
    assert_eq!(g.first_channel(), vec![7, 9]);
}

#[test]
fn rejects_malformed_files() {
    assert_eq!(decode(b"not a png"), Err(PngError::Malformed));
    let good = make_png(1, 1, 0, &[0, 5]);
    assert!(decode(&good[..good.len() - 30]).is_err());
    assert_eq!(decode(&good[..33]), Err(PngError::MissingChunk));
    assert_eq!(
        decode(&make_png(0, 1, 0, &[0, 5])),
        Err(PngError::BadDimensions)
    );
    assert_eq!(
        decode(&make_png(1, 1, 3, &[0, 5])),
        Err(PngError::Unsupported)
    );
    assert_eq!(decode(&make_png(1, 1, 0, &[9, 5])), Err(PngError::BadData));
    assert_eq!(decode(&make_png(1, 2, 0, &[0, 5])), Err(PngError::BadData));
}

#[test]
fn huge_header_is_rejected_without_allocating() {
    let r = decode(&make_png(u32::MAX, u32::MAX, 6, &[0]));
    assert_eq!(r, Err(PngError::BadDimensions));
}

#[test]
fn fuzzed_bytes_never_panic() {
    let base = make_png(3, 3, 2, &[0; 30]);
    let mut rng = Rng::new(99);
    for _ in 0..3000 {
        let mut f = base.clone();
        for _ in 0..1 + rng.below(4) {
            let i = rng.below(f.len() as u32) as usize;
            f[i] = rng.below(256) as u8;
        }
        let cut = rng.below(f.len() as u32 + 1) as usize;
        let _ = decode(&f[..cut]);
    }
}
