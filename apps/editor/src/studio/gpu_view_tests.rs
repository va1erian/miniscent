//! The 3D pane's scene on the GPU, against itself in software. Each test passes
//! with nothing to prove on a machine with no GPU: `mscent-gpu`'s parity test is
//! what insists on one.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use miniscent_game::SessionConfig;
use mscent_math::Vec3;
use mscent_render::{BakeCache, Framebuffer, View};

use super::gpu_view::Devices;
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

/// What the GPU cannot draw yet must not reach it: this has to be software's own
/// bytes, not a level with holes in it.
#[test]
fn a_level_with_viewports_stays_on_the_software_renderer() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/levels/facing.msl"
    );
    let bytes = std::fs::read(path).expect("the demo level");
    let level = mscent_world::format::read(&bytes).expect("it loads");
    let doc = Document::from_level(&level);
    let req = request(160, 90, Vec::new());
    let a = LevelScene::new(built(&doc))
        .render(&req)
        .expect("a frame")
        .clone();
    let mut gpu = LevelScene::on_gpu(built(&doc));
    assert_eq!(gpu.render(&req).expect("a frame").pixels(), a.pixels());
}

/// A device lost under a scene, as a driver reset loses it: that frame and the
/// next are software's own bytes, and the view says why. The device is the
/// test's own, so the tests beside it keep theirs.
#[test]
fn a_lost_device_leaves_the_pane_to_software_and_says_why() {
    let Ok(device) = mscent_gpu::Gpu::new() else {
        return;
    };
    let doc = showcase();
    let req = request(160, 90, Vec::new());
    let software = LevelScene::new(built(&doc))
        .render(&req)
        .expect("a frame")
        .clone();
    let mut scene = LevelScene::on_own_gpu(built(&doc), device.clone());
    let first = scene.render(&req).expect("a frame").clone();
    assert_ne!(first.pixels(), software.pixels(), "drawn on the GPU");
    assert_eq!(scene.gpu_failure(), None);
    device.destroy();
    for _ in 0..2 {
        let after = scene.render(&req).expect("a frame");
        assert_eq!(after.pixels(), software.pixels());
    }
    let why = scene.gpu_failure().expect("the GPU given up");
    assert!(why.contains("lost"), "{why}");
}

/// The editor's device, lost: opened again the first time, and the second time
/// an error that says why, which leaves every scene after it to software.
#[test]
fn a_lost_device_is_opened_again_once() {
    let open = || mscent_gpu::Gpu::new().map_err(|e| e.to_string());
    let Ok(first) = open() else {
        return;
    };
    let devices = Devices::new();
    let got = devices.get(|| Ok(first.clone())).expect("the first");
    assert!(!devices.can_reopen());
    got.destroy();
    assert!(devices.can_reopen());
    let second = devices.get(open).expect("opened again");
    assert_eq!(second.lost(), None);
    assert!(!devices.can_reopen());
    second.destroy();
    assert!(!devices.can_reopen());
    let why = devices.get(open).err().expect("not again");
    assert!(why.contains("lost"), "{why}");
    assert_eq!(devices.get(open).err(), Some(why), "and it stays gone");
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
