//! Feasibility bench: cost per pixel of baked-lightmap texturing with and without bump mapping.
//!
//! Integer-only, scalar, no unsafe, no deps, so the default (SSE2 baseline) build is what a
//! LazyOS binary would get. Content is synthetic; timing does not depend on texel values.
//!
//!   A  albedo * bilinear lightmap colour                         (Quake/Unreal-style baseline)
//!   B  A + per-pixel bump: normal-map fetch, bilinear baked light direction, N.L per pixel
//!   C  B but light colour/direction evaluated at the ends of 16px spans and stepped linearly
//!   S  A but with the light colour stepped per 16px span, the like-for-like baseline for C
//!
//! cargo run --release --example bump_bench

use std::hint::black_box;
use std::time::Instant;

const W: usize = 640;
const H: usize = 360;
const TEX: usize = 128;
const LM: usize = 64;
const SPAN: usize = 16;

struct Data {
    albedo: Vec<u32>,     // RGBA8
    normal: Vec<[i8; 4]>, // tangent-space xyz, 127 = 1.0
    lm_col: Vec<[u8; 4]>, // baked light colour
    lm_dir: Vec<[i8; 4]>, // baked dominant light direction, tangent space
}

fn make_data() -> Data {
    let mut s = 0x9E3779B9u32;
    let mut rnd = move || {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        s
    };
    let albedo = (0..TEX * TEX).map(|_| rnd() | 0xFF00_0000).collect();
    let normal = (0..TEX * TEX)
        .map(|_| [(rnd() % 40) as i8 - 20, (rnd() % 40) as i8 - 20, 120, 0])
        .collect();
    let lm_col = (0..LM * LM)
        .map(|_| [(rnd() % 200) as u8 + 40, (rnd() % 200) as u8 + 40, 200, 0])
        .collect();
    let lm_dir = (0..LM * LM)
        .map(|_| [(rnd() % 60) as i8 - 30, (rnd() % 60) as i8 - 30, 100, 0])
        .collect();
    Data {
        albedo,
        normal,
        lm_col,
        lm_dir,
    }
}

#[inline(always)]
fn lerp8(a: u32, b: u32, f: u32) -> u32 {
    (a * (256 - f) + b * f) >> 8
}

#[inline(always)]
fn bilerp_col(d: &Data, lx: u32, ly: u32) -> [u32; 3] {
    let (x0, y0) = ((lx >> 8) as usize & (LM - 1), (ly >> 8) as usize & (LM - 1));
    let (x1, y1) = ((x0 + 1) & (LM - 1), (y0 + 1) & (LM - 1));
    let (fx, fy) = (lx & 255, ly & 255);
    let t = [
        d.lm_col[y0 * LM + x0],
        d.lm_col[y0 * LM + x1],
        d.lm_col[y1 * LM + x0],
        d.lm_col[y1 * LM + x1],
    ];
    let mut o = [0u32; 3];
    for c in 0..3 {
        let a = lerp8(t[0][c] as u32, t[1][c] as u32, fx);
        let b = lerp8(t[2][c] as u32, t[3][c] as u32, fx);
        o[c] = lerp8(a, b, fy);
    }
    o
}

#[inline(always)]
fn bilerp_dir(d: &Data, lx: u32, ly: u32) -> [i32; 3] {
    let (x0, y0) = ((lx >> 8) as usize & (LM - 1), (ly >> 8) as usize & (LM - 1));
    let (x1, y1) = ((x0 + 1) & (LM - 1), (y0 + 1) & (LM - 1));
    let (fx, fy) = ((lx & 255) as i32, (ly & 255) as i32);
    let t = [
        d.lm_dir[y0 * LM + x0],
        d.lm_dir[y0 * LM + x1],
        d.lm_dir[y1 * LM + x0],
        d.lm_dir[y1 * LM + x1],
    ];
    let mut o = [0i32; 3];
    for c in 0..3 {
        let a = (t[0][c] as i32 * (256 - fx) + t[1][c] as i32 * fx) >> 8;
        let b = (t[2][c] as i32 * (256 - fx) + t[3][c] as i32 * fx) >> 8;
        o[c] = (a * (256 - fy) + b * fy) >> 8;
    }
    o
}

#[inline(always)]
fn shade(albedo: u32, light: [u32; 3], k: u32) -> u32 {
    // light 0..255 per channel, k 0..256 extra factor
    let r = ((((albedo & 255) * light[0]) >> 8) * k) >> 8;
    let g = (((((albedo >> 8) & 255) * light[1]) >> 8) * k) >> 8;
    let b = (((((albedo >> 16) & 255) * light[2]) >> 8) * k) >> 8;
    0xFF00_0000 | (b.min(255) << 16) | (g.min(255) << 8) | r.min(255)
}

#[inline(always)]
fn uv(x: usize, y: usize, f: usize) -> (u32, u32, u32, u32) {
    // fake affine mapping that changes per frame so nothing is hoisted
    let u = (x as u32 * 41 + y as u32 * 13 + f as u32 * 7) & 0xFFFF;
    let v = (x as u32 * 9 + y as u32 * 37 + f as u32 * 3) & 0xFFFF;
    let lx = (x as u32 * 25 + f as u32) & 0x3FFF;
    let ly = (y as u32 * 45 + f as u32) & 0x3FFF;
    (u, v, lx, ly)
}

fn frame_a(d: &Data, fb: &mut [u32], f: usize) {
    for y in 0..H {
        for x in 0..W {
            let (u, v, lx, ly) = uv(x, y, f);
            let a = d.albedo[(v as usize >> 9 & (TEX - 1)) * TEX + (u as usize >> 9 & (TEX - 1))];
            fb[y * W + x] = shade(a, bilerp_col(d, lx, ly), 256);
        }
    }
}

#[inline(always)]
fn bump_k(n: [i8; 4], l: [i32; 3]) -> u32 {
    let dot = (n[0] as i32 * l[0] + n[1] as i32 * l[1] + n[2] as i32 * l[2]) >> 7; // ~0..127
    // 25% flat term keeps surfaces from going black; the rest follows N.L (remapped so flat == 256)
    (dot.clamp(0, 255) as u32 * 2).min(511)
}

fn frame_b(d: &Data, fb: &mut [u32], f: usize) {
    for y in 0..H {
        for x in 0..W {
            let (u, v, lx, ly) = uv(x, y, f);
            let ti = (v as usize >> 9 & (TEX - 1)) * TEX + (u as usize >> 9 & (TEX - 1));
            let k = bump_k(d.normal[ti], bilerp_dir(d, lx, ly));
            fb[y * W + x] = shade(d.albedo[ti], bilerp_col(d, lx, ly), k);
        }
    }
}

fn frame_spans<const BUMP: bool>(d: &Data, fb: &mut [u32], f: usize) {
    for y in 0..H {
        let mut x = 0;
        while x < W {
            // evaluate light at both span ends, step linearly across the span
            let (_, _, lx0, ly0) = uv(x, y, f);
            let (_, _, lx1, ly1) = uv(x + SPAN, y, f);
            let (c0, c1) = (bilerp_col(d, lx0, ly0), bilerp_col(d, lx1, ly1));
            let (d0, d1) = (bilerp_dir(d, lx0, ly0), bilerp_dir(d, lx1, ly1));
            for i in 0..SPAN {
                let t = (i * 16) as i32; // 0..240
                let col =
                    [0, 1, 2].map(|c| ((c0[c] as i32 * (256 - t) + c1[c] as i32 * t) >> 8) as u32);
                let dir = [0, 1, 2].map(|c| (d0[c] * (256 - t) + d1[c] * t) >> 8);
                let (u, v, _, _) = uv(x + i, y, f);
                let ti = (v as usize >> 9 & (TEX - 1)) * TEX + (u as usize >> 9 & (TEX - 1));
                let k = if BUMP { bump_k(d.normal[ti], dir) } else { 256 };
                fb[y * W + x + i] = shade(d.albedo[ti], col, k);
            }
            x += SPAN;
        }
    }
}

fn frame_c(d: &Data, fb: &mut [u32], f: usize) {
    frame_spans::<true>(d, fb, f);
}

fn frame_s(d: &Data, fb: &mut [u32], f: usize) {
    frame_spans::<false>(d, fb, f);
}

fn bench(name: &str, d: &Data, fb: &mut [u32], run: fn(&Data, &mut [u32], usize)) -> f64 {
    for f in 0..10 {
        run(d, fb, f);
    }
    let frames = 300;
    let t = Instant::now();
    for f in 0..frames {
        run(black_box(d), fb, f);
        black_box(&fb);
    }
    let ms = t.elapsed().as_secs_f64() * 1000.0 / frames as f64;
    println!(
        "{name:<44} {ms:6.2} ms/frame  {:5.1} ns/px",
        ms * 1e6 / (W * H) as f64
    );
    ms
}

fn main() {
    let d = make_data();
    let mut fb = vec![0u32; W * H];
    println!("{W}x{H}, full-screen, overdraw 1.0, single thread");
    let a = bench("A  albedo * bilinear lightmap", &d, &mut fb, frame_a);
    let b = bench(
        "B  + bump, per-pixel bilinear light dir",
        &d,
        &mut fb,
        frame_b,
    );
    let s = bench(
        "S  A with light stepped per 16px span",
        &d,
        &mut fb,
        frame_s,
    );
    let c = bench(
        "C  + bump, light evaluated per 16px span",
        &d,
        &mut fb,
        frame_c,
    );
    println!(
        "bump overhead, like for like:  B vs A +{:.0}%   C vs S +{:.0}%",
        (b / a - 1.0) * 100.0,
        (c / s - 1.0) * 100.0
    );
    println!(
        "checksum {}",
        fb.iter().fold(0u32, |s, &p| s.wrapping_add(p))
    );
}
