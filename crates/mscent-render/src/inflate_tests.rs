use super::*;
use mscent_math::rng::Rng;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn fixed_huffman_with_backreferences() {
    // zlib.compress(b"abc" * 50, 9)
    let z = unhex("78da4b4c4a4e1c7c0800f07c396d");
    let out = zlib_decompress(&z, 1000).unwrap();
    assert_eq!(out, b"abc".repeat(50));
}

#[test]
fn dynamic_huffman_block_verified_by_adler() {
    // zlib.compress of 400 pseudo-random bytes over "abcdefgh "; the Adler-32
    // trailer is checked inside the decoder, so a wrong decode would fail.
    let z = unhex(
        "78da1d90c101c030080257613515d0fd2728e9230f8dc2e1b4faee967db55b27b6ab0ab5dc02ef4093a75af4\
         a88d05a54346b93b86fbf283f5d59576062c025cd8a7e8605e5567107b5e1790610ea6a1ea7e7b548ff34698\
         c8fbae23e64d5f6f9bb55373cf1dc7a87a4bc352b734ab09316f1eb69bcbeb15aebcaaa1e7170a1112a72b36\
         a19421dbadb797d0db81776f674416fa1486aa748adce91b4eef420860f39d49be9074fb2283b39e2e8390a3\
         b619f7782b359a39cb264cf2beb54bf8b443f2722359026ff00343368ef7",
    );
    let out = zlib_decompress(&z, 1000).unwrap();
    assert_eq!(out.len(), 400);
    assert_eq!(&out[..8], b"cbebhhhg");
    assert!(out.iter().all(|b| b"abcdefgh ".contains(b)));
}

#[test]
fn stored_block_round_trip() {
    let payload = b"hello";
    let mut z = vec![0x78, 0x01, 0x01, 5, 0, !5, 0xff];
    z.extend_from_slice(payload);
    z.extend_from_slice(&adler32(payload).to_be_bytes());
    assert_eq!(zlib_decompress(&z, 16).unwrap(), payload);
}

#[test]
fn rejects_bad_checksum_truncation_and_limits() {
    let mut z = unhex("78da4b4c4a4e1c7c0800f07c396d");
    assert_eq!(zlib_decompress(&z, 10), Err(InflateError::TooLarge));
    assert_eq!(
        zlib_decompress(&z[..z.len() - 2], 1000),
        Err(InflateError::Truncated)
    );
    *z.last_mut().unwrap() ^= 1;
    assert_eq!(zlib_decompress(&z, 1000), Err(InflateError::BadChecksum));
    assert_eq!(zlib_decompress(&[], 10), Err(InflateError::Truncated));
    assert_eq!(
        zlib_decompress(&[0x79, 0x9c], 10),
        Err(InflateError::BadHeader)
    );
}

#[test]
fn rejects_distance_before_start() {
    // Fixed block: literal-less stream whose first symbol is length 3, dist 1.
    // Bits: BFINAL=1, BTYPE=01, length code 257 (7 bits 0000001), dist code 0.
    let z = [0x78, 0x01, 0x03, 0x01, 0x00];
    assert_eq!(zlib_decompress(&z, 100), Err(InflateError::BadDistance));
}

#[test]
fn garbage_never_panics() {
    let mut rng = Rng::new(7);
    for _ in 0..2000 {
        let n = rng.below(64) as usize;
        let mut z: Vec<u8> = vec![0x78, 0x9c];
        z.extend((0..n).map(|_| rng.below(256) as u8));
        let _ = zlib_decompress(&z, 4096);
    }
}
