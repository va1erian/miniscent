//! Adversarial render checks: extreme geometry must not panic, and a banded
//! render must equal the full-frame render for any worker count.

use mscent_math::{Vec3, rng::Rng};
use mscent_render::{Camera, Fog, Framebuffer, Texture, TileRenderer, Vertex, draw_polygon_flat};

fn camera(w: u32, h: u32, near: f32) -> Camera {
    Camera::look_at(
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::new(1.0, 2.0, -5.0),
        Vec3::new(0.0, 1.0, 0.0),
        1.2,
        near,
        1.0e6,
        w,
        h,
    )
    .unwrap()
}

fn polys(seed: u64, n: usize) -> Vec<Vec<Vertex>> {
    let mut rng = Rng::new(seed);
    let mags = [1.0f32, 50.0, 1.0e4, 1.0e9, 1.0e18, 1.0e30];
    (0..n)
        .map(|_| {
            let m = mags[rng.below(mags.len() as u32) as usize];
            let k = 3 + rng.below(5) as usize;
            (0..k)
                .map(|_| {
                    let mut r = |s: f32| rng.range_f32(-1.0, 1.0) * s;
                    Vertex::new(Vec3::new(r(m), r(m), r(m) - 5.0), r(m), r(m))
                })
                .collect()
        })
        .collect()
}

fn tex() -> Texture {
    let px: Vec<u32> = (0..64 * 32)
        .map(|i| 0xff00_0000 | (i as u32).wrapping_mul(2654435761))
        .collect();
    Texture::new(64, 32, px).unwrap()
}

fn draw_all(fb: &mut Framebuffer, cam: &Camera, ps: &[Vec<Vertex>], t: &Texture) {
    fb.clear_sky(0xff20_4060, 0xff80_8080);
    let fog = Fog::new(0xff10_1010, 1.0, 400.0);
    for p in ps {
        draw_polygon_flat(fb, cam, p, t, 300, fog);
    }
}

#[test]
fn extreme_geometry_is_panic_free_and_bands_match_full_frame() {
    let t = tex();
    for (seed, near) in [(1u64, 0.1f32), (2, 1.0e-6), (3, 1.0e-20), (4, 5.0)] {
        let (w, h) = (97, 61);
        let cam = camera(w, h, near);
        let ps = polys(seed, 400);
        let mut full = Framebuffer::new(w, h).unwrap();
        draw_all(&mut full, &cam, &ps, &t);
        for (rows, workers) in [(1, 1), (7, 1), (7, 4), (61, 3)] {
            let mut tr = TileRenderer::new(w, h, rows, workers).unwrap();
            let mut out = Framebuffer::new(w, h).unwrap();
            tr.render(&mut out, &|b| draw_all(b, &cam, &ps, &t))
                .unwrap();
            assert_eq!(
                out.pixels(),
                full.pixels(),
                "seed {seed} rows {rows} workers {workers}"
            );
        }
    }
}

fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = (body.len() as u32).to_be_bytes().to_vec();
    v.extend_from_slice(kind);
    v.extend_from_slice(body);
    v.extend_from_slice(&[0; 4]);
    v
}

#[test]
fn hostile_png_and_zlib_streams_never_panic() {
    let mut rng = Rng::new(99);
    for round in 0..4000 {
        let (w, h) = (1 + rng.below(40), 1 + rng.below(40));
        let mut ihdr = [0u8; 13];
        ihdr[..4].copy_from_slice(&w.to_be_bytes());
        ihdr[4..8].copy_from_slice(&h.to_be_bytes());
        ihdr[8] = 8;
        ihdr[9] = [0, 2, 6][rng.below(3) as usize];
        let mut z = vec![0x78, 0x9c];
        let n = rng.below(200) as usize;
        z.extend((0..n).map(|_| {
            // Bias toward small values so block headers and codes get hit.
            let r = rng.below(256) as u8;
            if round % 2 == 0 { r } else { r & 0x0f }
        }));
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        png.extend(chunk(b"IHDR", &ihdr));
        png.extend(chunk(b"IDAT", &z));
        png.extend(chunk(b"IEND", &[]));
        let _ = mscent_render::png::decode(&png);
        let cut = rng.below(png.len() as u32) as usize;
        let _ = mscent_render::png::decode(&png[..cut]);
        let _ = mscent_render::inflate::zlib_decompress(&z, 1 << 16);
    }
}
