//! One showcase level per use case holds its triangle budget, shows no sky from
//! a pose in air, and comes out the same from the same seed.
//!
//! `MSCENT_SHOTS=dir` also writes a picture of each, and `MSCENT_BLESS=1` writes
//! the levels into `assets/levels` for `voxel_shot` to draw.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mscent_render::png_encode::encode_png;
use mscent_render::voxel::MAX_VOXEL_TRIS;
use mscent_render::{
    BuildConfig, Compiled, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
};
use mscent_world::format;

use super::showcase::{Case, Scene, build};

const SEED: u32 = 11;
/// The fuchsia that stands in for sky, so any hole in a surface shows.
const HOLE: u32 = 0xffff_00ff;
const SIZE: (u32, u32) = (640, 360);
/// Frames the governor gets to settle.
const SETTLE: usize = 40;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn renderer(scene: &Scene) -> LevelRenderer {
    let table = MaterialTable::load_dir(&assets().join("textures")).expect("textures");
    let cfg = BuildConfig {
        luxel_size: 0.5,
        sky_top: HOLE,
        sky_bottom: HOLE,
        fog_start: 200.0,
        fog_end: 400.0,
        ..BuildConfig::default()
    };
    let c = Compiled::new(&scene.level, &cfg).expect("compiles");
    let map = Arc::new(c.draft().expect("drafts"));
    c.into_renderer(map, table).expect("assembles")
}

fn view(scene: &Scene) -> View {
    let to = scene.target - scene.eye;
    let yaw = (-to.x).atan2(to.y);
    let pitch = to.z.atan2(to.x.hypot(to.y));
    View::from_angles(scene.eye, yaw, pitch, 0.0)
}

fn bytes(case: Case, seed: u32) -> Vec<u8> {
    format::write(&build(case, seed).level)
}

#[test]
fn the_same_seed_gives_the_same_bytes_and_another_seed_gives_other_ones() {
    for case in Case::ALL {
        let a = bytes(case, SEED);
        assert_eq!(a, bytes(case, SEED), "{}", case.name());
        assert_ne!(a, bytes(case, SEED + 1), "{}", case.name());
        let level = format::read(&a).expect("the saved file reads back");
        assert_eq!(level.volumes.len(), 1, "{}", case.name());
    }
}

#[test]
fn every_showcase_holds_its_budget_and_shows_no_sky() {
    let shots = std::env::var_os("MSCENT_SHOTS").map(PathBuf::from);
    for case in Case::ALL {
        let scene = build(case, SEED);
        let r = renderer(&scene);
        let (w, h) = SIZE;
        let mut fb = Framebuffer::new(w, h).expect("fb");
        let view = view(&scene);
        let mut stats = r
            .render(&mut fb, &view, Quality::High, &[], None)
            .expect("renders");
        for _ in 0..SETTLE {
            stats = r
                .render(&mut fb, &view, Quality::High, &[], None)
                .expect("renders");
        }
        let holes = fb.pixels().iter().filter(|&&p| p == HOLE).count();
        println!(
            "{}: {} voxel triangles, {holes} hole pixels",
            case.name(),
            stats.voxel_triangles
        );
        assert!(stats.voxel_triangles > 0, "{} drew no voxel", case.name());
        assert!(
            stats.voxel_triangles <= MAX_VOXEL_TRIS,
            "{}: {} triangles",
            case.name(),
            stats.voxel_triangles
        );
        assert_eq!(holes, 0, "{} shows the sky", case.name());
        if let Some(dir) = &shots {
            std::fs::create_dir_all(dir).expect("directory");
            let png = encode_png(w, h, fb.pixels()).expect("encodes");
            std::fs::write(dir.join(format!("{}.png", case.name())), png).expect("writes");
        }
    }
}

#[test]
fn the_saved_showcase_levels_load_and_have_their_volume() {
    let dir = assets().join("levels");
    for case in Case::ALL {
        let path = dir.join(case.file());
        if std::env::var_os("MSCENT_BLESS").is_some() {
            std::fs::write(&path, bytes(case, SEED)).expect("writes the level");
        }
        let saved = std::fs::read(&path).unwrap_or_default();
        let level = format::read(&saved);
        let level = level.unwrap_or_else(|e| {
            panic!(
                "{}: {e:?}; run with MSCENT_BLESS=1 to write it",
                path.display()
            )
        });
        assert_eq!(level.volumes.len(), 1, "{}", case.name());
    }
}
