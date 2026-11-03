//! `Finish::Fine` against `Finish::Reference` on the GPU: the same picture, and
//! nearer what the scene looks like, which is a fine frame drawn four times larger
//! each way and averaged down. One no nearer has lost the finish; one that is not
//! the same picture in the mean has changed the shading, which only the reference
//! is held to, in `tests/parity.rs`.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/compare.rs"]
#[allow(dead_code, reason = "shared with the parity test")]
mod compare;
#[path = "support/scenes.rs"]
#[allow(dead_code, reason = "shared with the parity test")]
mod scenes;

use compare::Frame;
use mscent_gpu::{Finish, Gpu, GpuError, GpuRenderer};
use mscent_math::epsilon::{
    FINE_DEPTH_AGREEMENT, FINE_EDGES_NEARER, FINE_MEAN_LEVELS, FINE_NEARER, PARITY_DEPTH_RELATIVE,
    PARITY_EDGE_DEPTH,
};
use mscent_render::{Effects, FrameInput, Framebuffer, Quality};
use scenes::{Framing, Pose, Scene};

const WIDTH: u32 = 320;
const HEIGHT: u32 = 180;
/// How many times larger each way the frame the truth is made from is.
const SUPERSAMPLE: u32 = 4;

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(why @ (GpuError::NoAdapter(_) | GpuError::NoDevice(_))) => {
            assert!(
                std::env::var_os("MSCENT_REQUIRE_GPU").is_none(),
                "MSCENT_REQUIRE_GPU is set: {why}"
            );
            eprintln!("skipped: {why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}

fn draw(gpu: &Gpu, scene: &Scene, framing: &Framing, finish: Finish, scale: u32) -> Frame {
    let level = scene.software(None);
    let mut renderer =
        GpuRenderer::with_finish(gpu.clone(), level.assets(), finish).expect("it fits");
    let mut fb = Framebuffer::new(WIDTH * scale, HEIGHT * scale).expect("a framebuffer");
    let input = FrameInput {
        view: &framing.view,
        quality: Quality::High,
        actors: &framing.actors,
        effects: Effects {
            poses: &framing.poses,
            ..Effects::default()
        },
        wants_depth: true,
    };
    renderer
        .draw(level.assets(), &mut fb, &input)
        .expect("a frame");
    assert_eq!(renderer.take_error(), None, "{finish:?}");
    Frame::of(&fb)
}

fn channels(pixel: u32) -> [i64; 3] {
    [16, 8, 0].map(|s| i64::from((pixel >> s) & 255))
}

/// Mean difference per channel, in levels.
fn mean_error(a: &Frame, b: &Frame) -> f64 {
    let sum: i64 = (a.pixels.iter().zip(&b.pixels))
        .map(|(&x, &y)| {
            let (x, y) = (channels(x), channels(y));
            (0..3).map(|c| (x[c] - y[c]).abs()).sum::<i64>()
        })
        .sum();
    sum as f64 / (a.pixels.len() * 3) as f64
}

/// What the scene looks like at `framing`: a fine frame at four times the size
/// each way, each 4x4 block averaged.
fn truth(gpu: &Gpu, scene: &Scene, framing: &Framing) -> Frame {
    let big = draw(gpu, scene, framing, Finish::Fine, SUPERSAMPLE);
    let mut pixels = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            let mut sum = [0i64; 3];
            for dy in 0..SUPERSAMPLE as usize {
                for dx in 0..SUPERSAMPLE as usize {
                    let at = (y * 4 + dy) * big.width + x * 4 + dx;
                    let c = channels(big.pixels[at]);
                    (0..3).for_each(|i| sum[i] += c[i]);
                }
            }
            let n = i64::from(SUPERSAMPLE * SUPERSAMPLE);
            let [r, g, b] = sum.map(|v| ((v + n / 2) / n) as u32);
            pixels.push(0xff00_0000 | r << 16 | g << 8 | b);
        }
    }
    Frame {
        width: WIDTH as usize,
        height: HEIGHT as usize,
        pixels,
        depth: Vec::new(),
    }
}

/// The pixels of `frame` on a silhouette: where `1/z` jumps to a neighbour, as
/// the parity test reads an edge.
fn silhouette(frame: &Frame) -> Vec<bool> {
    let (w, h) = (frame.width, frame.height);
    let jumps = |a: f32, b: f32| (a - b).abs() > a.max(b) * PARITY_EDGE_DEPTH;
    (0..w * h)
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let d = frame.depth[i];
            (x + 1 < w && jumps(d, frame.depth[i + 1]))
                || (y + 1 < h && jumps(d, frame.depth[i + w]))
        })
        .collect()
}

/// Mean difference per channel over the pixels `among` marks, in levels.
fn mean_error_among(a: &Frame, b: &Frame, among: &[bool]) -> f64 {
    let (mut sum, mut n) = (0i64, 0usize);
    for ((&x, &y), _) in (a.pixels.iter().zip(&b.pixels))
        .zip(among)
        .filter(|(_, m)| **m)
    {
        let (x, y) = (channels(x), channels(y));
        sum += (0..3).map(|c| (x[c] - y[c]).abs()).sum::<i64>();
        n += 3;
    }
    sum as f64 / n.max(1) as f64
}

/// The share of pixels with geometry in both whose depths agree.
fn depth_agreement(a: &Frame, b: &Frame) -> f64 {
    let (mut both, mut agree) = (0usize, 0usize);
    for (&x, &y) in a.depth.iter().zip(&b.depth) {
        if x > 0.0 && y > 0.0 {
            both += 1;
            agree += usize::from((x - y).abs() <= x * PARITY_DEPTH_RELATIVE);
        }
    }
    agree as f64 / both.max(1) as f64
}

#[test]
fn a_fine_frame_is_the_same_picture_smoother() {
    let Some(gpu) = gpu() else { return };
    let showcase = Scene::showcase();
    for pose in [
        Pose::DownTheHall,
        Pose::FarFromMaterial("floor"),
        Pose::AtActor,
    ] {
        let framing = showcase.framing(pose);
        let reference = draw(&gpu, &showcase, &framing, Finish::Reference, 1);
        let fine = draw(&gpu, &showcase, &framing, Finish::Fine, 1);
        let truth = truth(&gpu, &showcase, &framing);
        let error = mean_error(&reference, &fine);
        let (was, now) = (mean_error(&reference, &truth), mean_error(&fine, &truth));
        let edges = silhouette(&reference);
        let (edge_was, edge_now) = (
            mean_error_among(&reference, &truth, &edges),
            mean_error_among(&fine, &truth, &edges),
        );
        let agreement = depth_agreement(&reference, &fine);
        eprintln!("apart {error:.2}; from the truth {was:.2} to {now:.2}");
        eprintln!("  on edges {edge_was:.2} to {edge_now:.2}; depth agrees {agreement:.3}");
        assert!(error < FINE_MEAN_LEVELS, "the same picture: {error}");
        assert!(now < was * FINE_NEARER, "nearer the truth: {was} to {now}");
        assert!(
            edge_now < edge_was * FINE_EDGES_NEARER,
            "on edges: {edge_was} to {edge_now}"
        );
        assert!(
            agreement > FINE_DEPTH_AGREEMENT,
            "the same depth: {agreement}"
        );
    }
}
