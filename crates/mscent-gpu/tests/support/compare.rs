//! How alike two frames are. Pixels on a silhouette are left out, as the two
//! renderers may fairly disagree about who owns them; which pixels those are
//! comes from the software frame alone, so a hole in the other cannot excuse
//! itself.

use mscent_math::epsilon::{
    PARITY_BLOCK_LEVELS, PARITY_DEPTH_RELATIVE, PARITY_EDGE_DEPTH, PARITY_MEAN_LEVELS,
};
use mscent_render::Framebuffer;

/// Side of the blocks whose mean error is looked at, in pixels.
const BLOCK: usize = 8;

/// A frame copied out of its framebuffer.
pub struct Frame {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u32>,
    pub depth: Vec<f32>,
}

impl Frame {
    pub fn of(fb: &Framebuffer) -> Self {
        let (width, height) = (fb.width() as usize, fb.height() as usize);
        let at = |i: usize| ((i % width) as i32, (i / width) as i32);
        let depth = (0..width * height)
            .map(|i| fb.depth_at(at(i).0, at(i).1).unwrap_or(0.0))
            .collect();
        Self {
            width,
            height,
            pixels: fb.pixels().to_vec(),
            depth,
        }
    }

    /// Whether a surface edge touches pixel `(x, y)`.
    fn on_edge(&self, x: usize, y: usize) -> bool {
        let here = self.depth[y * self.width + x];
        let rows = y.saturating_sub(1)..=(y + 1).min(self.height - 1);
        rows.into_iter().any(|ny| {
            let columns = x.saturating_sub(1)..=(x + 1).min(self.width - 1);
            columns.into_iter().any(|nx| {
                let there = self.depth[ny * self.width + nx];
                (here == 0.0) != (there == 0.0)
                    || (here - there).abs() > PARITY_EDGE_DEPTH * here.max(there)
            })
        })
    }
}

/// How far apart two frames are, away from silhouettes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Report {
    /// Mean error per colour channel, in levels of 255.
    pub mean: f32,
    /// The mean error of the worst block: a shadow that moved shows here, when
    /// a mean over the frame would average it away.
    pub worst_block: f32,
    /// Mean relative difference of `1/z`, counting nothing drawn against drawn
    /// as a whole difference.
    pub depth: f32,
}

impl Report {
    pub fn alike(&self) -> bool {
        self.mean <= PARITY_MEAN_LEVELS
            && self.worst_block <= PARITY_BLOCK_LEVELS
            && self.depth <= PARITY_DEPTH_RELATIVE
    }
}

fn channel_error(a: u32, b: u32) -> u32 {
    [16, 8, 0]
        .iter()
        .map(|s| ((a >> s) & 255).abs_diff((b >> s) & 255))
        .sum()
}

fn depth_error(a: f32, b: f32) -> f32 {
    if a == b {
        return 0.0;
    }
    (a - b).abs() / a.max(b)
}

/// Compares `other` with the `reference` frame of the same size.
pub fn compare(reference: &Frame, other: &Frame) -> Report {
    let (w, h) = (reference.width, reference.height);
    let blocks_across = w.div_ceil(BLOCK);
    let mut blocks = vec![(0u64, 0u64); blocks_across * h.div_ceil(BLOCK)];
    let (mut total, mut depth, mut counted) = (0u64, 0.0f64, 0u64);
    for y in 0..h {
        for x in 0..w {
            if reference.on_edge(x, y) {
                continue;
            }
            let i = y * w + x;
            let error = u64::from(channel_error(reference.pixels[i], other.pixels[i]));
            total += error;
            depth += f64::from(depth_error(reference.depth[i], other.depth[i]));
            counted += 1;
            let block = &mut blocks[y / BLOCK * blocks_across + x / BLOCK];
            block.0 += error;
            block.1 += 1;
        }
    }
    let per_channel = |sum: u64, n: u64| sum as f32 / (3 * n.max(1)) as f32;
    let worst_block = blocks.iter().map(|&(sum, n)| per_channel(sum, n));
    Report {
        mean: per_channel(total, counted),
        worst_block: worst_block.fold(0.0, f32::max),
        depth: (depth / counted.max(1) as f64) as f32,
    }
}

#[test]
fn a_frame_is_alike_to_itself() {
    let frame = flat_frame(0xff40_6080, 0.5);
    assert_eq!(compare(&frame, &frame), Report::default());
}

#[test]
fn a_small_patch_fails_on_its_block_though_the_mean_barely_moves() {
    let frame = flat_frame(0xff40_6080, 0.5);
    let mut patched = flat_frame(0xff40_6080, 0.5);
    for y in 8..16 {
        for x in 8..16 {
            patched.pixels[y * patched.width + x] = 0xff50_7090;
        }
    }
    let report = compare(&frame, &patched);
    assert!(report.mean <= PARITY_MEAN_LEVELS, "{report:?}");
    assert!(!report.alike(), "{report:?}");
}

#[test]
fn a_hole_fails_on_depth_even_where_the_colours_agree() {
    let frame = flat_frame(0xff40_6080, 0.5);
    let holed = flat_frame(0xff40_6080, 0.0);
    assert!(!compare(&frame, &holed).alike());
}

#[test]
fn a_silhouette_is_forgiven_but_only_the_reference_decides_where() {
    let mut with_edge = flat_frame(0xff40_6080, 0.5);
    for y in 0..with_edge.height {
        with_edge.depth[y * with_edge.width + 80] = 0.0;
        with_edge.pixels[y * with_edge.width + 80] = 0xff00_0000;
    }
    let plain = flat_frame(0xff40_6080, 0.5);
    assert!(compare(&with_edge, &plain).alike());
    assert!(!compare(&plain, &with_edge).alike());
}

fn flat_frame(pixel: u32, depth: f32) -> Frame {
    let (width, height) = (160, 90);
    Frame {
        width,
        height,
        pixels: vec![pixel; width * height],
        depth: vec![depth; width * height],
    }
}
