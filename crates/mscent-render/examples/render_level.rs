//! Renders the showcase level and a generated level to PNG files.
//!
//! `cargo run --release -p mscent-render --example render_level -- <out_dir>`
//! writes `showcase_*.png`, a low/high quality pair of the same view, a
//! rolled view, pillar shadow views and `generated_seed7.png`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use mscent_math::Vec3;
use mscent_render::png_encode::encode_png;
use mscent_render::{
    BuildConfig, Framebuffer, LevelRenderer, MaterialTable, Overlay, Quality, View,
};
use mscent_world::{Entity, GenParams, Level, generate_level, showcase_level};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;

fn textures() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("load assets/textures")
}

fn build(level: &Level) -> LevelRenderer {
    let t = Instant::now();
    let r = LevelRenderer::from_level(level, textures(), &BuildConfig::default())
        .expect("level builds");
    println!(
        "built {} faces, {} luxel atlas in {:.2}s",
        r.scene().faces().len(),
        r.lightmap().atlas().width() * r.lightmap().atlas().height(),
        t.elapsed().as_secs_f32()
    );
    r
}

struct Shots {
    out: PathBuf,
}

impl Shots {
    fn take(&self, name: &str, r: &LevelRenderer, view: &View, quality: Quality) {
        let mut fb = Framebuffer::new(WIDTH, HEIGHT).expect("framebuffer");
        // A first pass measures the frame time that the HUD then reports.
        // The fastest of several passes: other processes only add time.
        let ms = (0..9)
            .map(|_| {
                let t = Instant::now();
                r.render(&mut fb, view, quality, r.actors(), None)
                    .expect("render");
                t.elapsed().as_secs_f32() * 1000.0
            })
            .fold(f32::MAX, f32::min);
        let hud = Overlay {
            health: 82,
            max_health: 100,
            fps: 1000.0 / ms.max(0.01),
        };
        let stats = r
            .render(&mut fb, view, quality, r.actors(), Some(&hud))
            .expect("render");
        let png = encode_png(WIDTH, HEIGHT, fb.pixels()).expect("encode");
        std::fs::write(self.out.join(format!("{name}.png")), png).expect("write png");
        println!(
            "{name}: {:?} {ms:.1} ms, {} faces, {} pixels",
            quality,
            stats.drawn.len(),
            stats.pixels
        );
    }
}

fn at(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

fn main() {
    let out: PathBuf = std::env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/shots/render_level"), PathBuf::from);
    std::fs::create_dir_all(&out).expect("create output dir");
    let shots = Shots { out };
    let (pi, hp) = (core::f32::consts::PI, core::f32::consts::FRAC_PI_2);

    let showcase = build(&showcase_level());
    let high = Quality::High;
    let start = View::from_angles(at(3.0, 3.0, 2.5), -pi / 4.0, 0.0, 0.0);
    shots.take("showcase_1_start_pillar", &showcase, &start, high);
    let hall = View::from_angles(at(25.0, 10.0, 2.0), -hp, 0.05, 0.0);
    shots.take("showcase_2_corridor_to_blue_hall", &showcase, &hall, high);
    let steps = View::from_angles(at(34.0, 2.0, 3.5), -0.7, -0.15, 0.0);
    shots.take("showcase_3_steps", &showcase, &steps, high);
    let cave = View::from_angles(at(33.0, 36.0, 3.0), -0.8, 0.0, 0.0);
    shots.take("showcase_4_cave_goal", &showcase, &cave, high);
    let ramp = View::from_angles(at(21.0, 48.0, 3.0), hp, -0.1, 0.0);
    shots.take("showcase_5_red_ramp", &showcase, &ramp, high);
    let rolled = View::from_angles(at(3.0, 3.0, 2.5), -pi / 4.0, 0.1, 30f32.to_radians());
    shots.take("showcase_6_rolled_30deg", &showcase, &rolled, high);
    let wall = View::from_angles(at(8.0, 17.0, 3.0), 0.4, 0.05, 0.0);
    shots.take("showcase_7_quality_low", &showcase, &wall, Quality::Low);
    shots.take("showcase_8_quality_high", &showcase, &wall, high);

    // The lamp is at (6, 6) and the pillar at (11..13, 11..13): its shadow
    // falls on the floor and walls to the north east of it.
    let shadow = View::from_angles(at(1.5, 9.0, 6.5), -1.0, -0.4, 0.0);
    shots.take("showcase_9_pillar_shadow", &showcase, &shadow, high);
    let behind = View::from_angles(at(14.0, 1.0, 6.0), -0.13, -0.37, 0.0);
    shots.take("showcase_9b_shadow_from_lamp", &showcase, &behind, high);
    let across = View::from_angles(at(18.5, 18.5, 4.0), 3.0 * pi / 4.0, -0.2, 0.0);
    shots.take("showcase_9c_shadow_across", &showcase, &across, high);
    let goal = View::from_angles(at(53.0, 38.0, 2.0), 0.0, 0.1, 0.0);
    shots.take("showcase_10_goal", &showcase, &goal, high);

    let level = generate_level(7, &GenParams::default());
    let (pos, yaw) = level
        .entities
        .iter()
        .find_map(|e| match *e {
            Entity::PlayerStart { pos, yaw } => Some((pos, yaw)),
            _ => None,
        })
        .unwrap_or((at(0.0, 0.0, 2.0), 0.0));
    let generated = build(&level);
    let view = View::from_angles(pos + at(0.0, 0.0, 0.5), yaw, 0.0, 0.0);
    shots.take("generated_seed7", &generated, &view, high);
}
