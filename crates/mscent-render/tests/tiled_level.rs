//! A real level frame in parallel bands: enemies, goal, HUD and every quality
//! give the pixels and depth of one full-height band on one worker.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Overlay, Quality, View,
};
use mscent_world::showcase_level;

const W: u32 = 320;
const H: u32 = 180;
const HUD: Overlay = Overlay {
    health: 60,
    max_health: 100,
    fps: 60.0,
};

fn renderer() -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .expect("builds")
}

fn views(r: &LevelRenderer) -> Vec<View> {
    let room = (0..4)
        .map(|i| View::from_angles(Vec3::new(3.0, 3.0, 2.5), i as f32 * 1.57 - 0.8, 0.0, 0.0));
    // Close enough to an actor that its mesh crosses many bands.
    let close = r
        .actors()
        .iter()
        .map(|a| View::from_angles(a.pos + Vec3::new(0.0, -3.0, 0.5), 0.0, -0.1, 0.0));
    room.chain(close).collect()
}

type Frame = (Vec<u32>, Vec<u32>);

fn frame(r: &LevelRenderer, view: &View, q: Quality, size: (u32, u32)) -> Frame {
    let mut fb = Framebuffer::new(size.0, size.1).expect("fb");
    r.render(&mut fb, view, q, r.actors(), Some(&HUD))
        .expect("frame");
    let depth = (0..size.1 as i32)
        .flat_map(|y| (0..size.0 as i32).map(move |x| (x, y)))
        .map(|(x, y)| fb.depth_at(x, y).expect("in range").to_bits())
        .collect();
    (fb.pixels().to_vec(), depth)
}

#[test]
fn every_tiling_gives_the_pixels_of_one_band_on_one_worker() {
    let mut r = renderer();
    let views = views(&r);
    assert!(views.len() > 4, "the showcase has actors");
    for q in [Quality::Low, Quality::Medium, Quality::High] {
        for (i, view) in views.iter().enumerate() {
            r.set_tiling(H, 1);
            let want = frame(&r, view, q, (W, H));
            for (rows, workers) in [(24, 1), (24, 4), (24, 16), (7, 3), (1, 8), (999, 2)] {
                r.set_tiling(rows, workers);
                let got = frame(&r, view, q, (W, H));
                assert!(
                    got == want,
                    "{q:?} view {i}, {rows} rows, {workers} workers"
                );
            }
        }
    }
}

#[test]
fn stats_do_not_depend_on_the_tiling() {
    let mut r = renderer();
    let mut stats = |rows, workers| {
        r.set_tiling(rows, workers);
        let mut fb = Framebuffer::new(W, H).expect("fb");
        let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.8, 0.0, 0.0);
        r.render(&mut fb, &view, Quality::Low, &[], None)
            .expect("frame")
    };
    let one = stats(H, 1);
    assert!(one.pixels > 0 && !one.drawn.is_empty());
    assert_eq!(one, stats(24, 4));
    assert_eq!(one, stats(1, 8));
}

/// The adaptive resolution changes the size between frames; a later frame must
/// show nothing of the old size.
#[test]
fn changing_the_size_between_frames_matches_a_fresh_renderer() {
    let r = renderer();
    let view = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.8, 0.0, 0.0);
    for size in [(W, H), (W / 2, H / 2), (W, H), (W + 37, H - 11)] {
        let got = frame(&r, &view, Quality::High, size);
        let want = frame(&renderer(), &view, Quality::High, size);
        assert!(got == want, "size {size:?}");
    }
}

#[test]
fn a_bad_view_draws_nothing_and_the_next_frame_is_fine() {
    let r = renderer();
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let bad = View::from_angles(Vec3::new(f32::NAN, 0.0, 0.0), 0.0, 0.0, 0.0);
    assert!(r.render(&mut fb, &bad, Quality::Low, &[], None).is_none());
    let ok = View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.8, 0.0, 0.0);
    assert!(r.render(&mut fb, &ok, Quality::Low, &[], None).is_some());
}
