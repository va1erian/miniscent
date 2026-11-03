//! The 3D pane's scene on the GPU, against itself in software. Each test passes
//! with nothing to prove on a machine with no GPU: `mscent-gpu`'s parity test is
//! what insists on one.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use miniscent_game::SessionConfig;
use mscent_math::Vec3;
use mscent_render::{BakeCache, Framebuffer, View};

use super::scene::LevelScene;
use super::worker::{Request, Scene};
use crate::build::{self, Built};
use crate::model::Document;

fn showcase() -> Document {
    Document::from_level(&mscent_world::showcase_level())
}

fn built(doc: &Document) -> Built {
    let cache = Arc::new(Mutex::new(BakeCache::new()));
    let job = build::start_preview(doc, &cache, &Default::default(), &SessionConfig::default())
        .expect("starts");
    loop {
        if let Some(result) = job.poll() {
            return result.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn request(w: u32, h: u32, highlights: Vec<Vec<Vec3>>) -> Request {
    Request {
        w,
        h,
        play: false,
        view: View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, -0.2, 0.0),
        banner: None,
        highlights: Arc::new(highlights),
    }
}

fn has_gpu() -> bool {
    mscent_gpu::Gpu::new().is_ok()
}

/// Mean error per channel, in levels.
fn mean_error(a: &Framebuffer, b: &Framebuffer) -> f64 {
    let sum: u64 = a
        .pixels()
        .iter()
        .zip(b.pixels())
        .map(|(&x, &y)| {
            [16, 8, 0]
                .iter()
                .map(|s| u64::from(((x >> s) & 255).abs_diff((y >> s) & 255)))
                .sum::<u64>()
        })
        .sum();
    sum as f64 / (a.pixels().len() * 3) as f64
}

#[test]
fn the_pane_shows_the_same_level_on_the_gpu() {
    if !has_gpu() {
        return;
    }
    let doc = showcase();
    let req = request(320, 180, Vec::new());
    let mut software = LevelScene::new(built(&doc));
    let mut gpu = LevelScene::on_gpu(built(&doc));
    let a = software.render(&req).expect("a frame").clone();
    let b = gpu.render(&req).expect("a frame");
    assert!(mean_error(&a, b) < 3.0, "{}", mean_error(&a, b));
    assert_ne!(a.pixels(), b.pixels(), "this frame did come from the GPU");
}

/// A highlight is hidden by what is in front of it, so it needs the frame's depth:
/// this floor quad is cut by the pillar in both pictures.
#[test]
fn a_highlight_on_the_gpu_is_cut_by_what_is_in_front() {
    if !has_gpu() {
        return;
    }
    let doc = showcase();
    let floor = |x, y| Vec3::new(x, y, 0.0);
    let quad = vec![vec![
        floor(0.0, 0.0),
        floor(60.0, 0.0),
        floor(60.0, 60.0),
        floor(0.0, 60.0),
    ]];
    let (plain, lit) = (request(320, 180, Vec::new()), request(320, 180, quad));
    let tinted = |scene: &mut LevelScene| {
        let before = scene.render(&plain).expect("a frame").clone();
        let after = scene.render(&lit).expect("a frame");
        let changed = |(a, b): (&u32, &u32)| a != b;
        before
            .pixels()
            .iter()
            .zip(after.pixels())
            .filter(|p| changed(*p))
            .count()
    };
    let software = tinted(&mut LevelScene::new(built(&doc)));
    let gpu = tinted(&mut LevelScene::on_gpu(built(&doc)));
    assert!(software > 0, "the quad is in view");
    assert!(
        gpu.abs_diff(software) * 20 < software,
        "{gpu} pixels tinted on the GPU, {software} in software"
    );
}

/// A level with viewports draws on the GPU too, vistas and cutouts and all, and
/// shows what software shows.
#[test]
fn a_level_with_viewports_draws_on_the_gpu() {
    if !has_gpu() {
        return;
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/levels/facing.msl"
    );
    let bytes = std::fs::read(path).expect("the demo level");
    let level = mscent_world::format::read(&bytes).expect("it loads");
    let start = level.player_start().expect("a player start");
    let doc = Document::from_level(&level);
    let mut req = request(320, 180, Vec::new());
    // Out over the edge, up into the sky, which is all vista.
    req.view = View::from_angles(start + Vec3::new(0.0, 0.0, 1.0), 0.0, 0.3, 0.0);
    let a = LevelScene::new(built(&doc))
        .render(&req)
        .expect("a frame")
        .clone();
    let mut gpu = LevelScene::on_gpu(built(&doc));
    let b = gpu.render(&req).expect("a frame");
    assert!(mean_error(&a, b) < 3.0, "{}", mean_error(&a, b));
    assert_ne!(a.pixels(), b.pixels(), "this frame did come from the GPU");
}

/// `cargo test --release -p mscent-editor pane_cost -- --ignored --nocapture`
#[test]
#[ignore = "a measurement, for a release build on a quiet machine"]
fn pane_cost() {
    let doc = showcase();
    let t = Instant::now();
    let mut scene = LevelScene::on_gpu(built(&doc));
    println!("build {:?}", t.elapsed());
    // Every edit brings a new scene on the same device: that first frame is what
    // an edit costs on top of the software path.
    scene.render(&request(1920, 1080, Vec::new()));
    for _ in 0..3 {
        let fresh = built(&doc);
        let mut next = LevelScene::on_gpu(fresh);
        let t = Instant::now();
        next.render(&request(1920, 1080, Vec::new()));
        println!("a new scene's first frame {:?}", t.elapsed());
    }
    for (w, h) in [(1920, 1080), (3840, 2160)] {
        for highlights in [Vec::new(), vec![vec![Vec3::ZERO; 3]]] {
            let req = request(w, h, highlights);
            let t = Instant::now();
            scene.render(&req);
            let first = t.elapsed();
            let mut costs: Vec<Duration> = (0..60)
                .map(|_| {
                    let t = Instant::now();
                    scene.render(&req);
                    t.elapsed()
                })
                .collect();
            costs.sort();
            println!(
                "{w}x{h} highlights {}: first {first:?}, median {:?}, worst {:?}",
                req.highlights.len(),
                costs[30],
                costs[59]
            );
        }
    }
}
