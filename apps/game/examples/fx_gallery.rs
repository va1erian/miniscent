//! Every combat effect at fixed times over the showcase, as contact sheets for
//! a person to judge, because neither `Blast` nor `Shatter` fires in play yet.
//! Prints what the worst frame of each costs.
//!
//! Run: `cargo run --release -p miniscent-game --example fx_gallery`
//! Writes one PNG per effect into `target/shots/fx/`: a row of frames over time.

use std::time::Instant;

use miniscent_game::assets;
use mscent_game::fx::budget::cost_of;
use mscent_game::fx::combat::{CombatFx, DRONE_TINT, Impact, ShotFx, ViewKick};
use mscent_math::{Aabb, Vec3};
use mscent_render::level::Effects;
use mscent_render::particles::SpriteBank;
use mscent_render::png_encode::encode_png;
use mscent_render::{Framebuffer, LevelRenderer, Quality, View, simd};
use mscent_world::materials::{BRICK, FLOOR, LIGHTPANEL, METAL, ROCK};
use mscent_world::showcase_level;

/// Size of one measured frame, the same as the design's tables.
const W: u32 = 640;
const H: u32 = 360;
const FRAME_NS: u64 = 16_666_667;
/// Seconds at which a frame goes on the sheet.
const TIMES: [f32; 6] = [0.03, 0.1, 0.25, 0.5, 1.0, 2.5];
/// How far before the effect the camera stands, and the most it looks for a wall.
const VIEW_DISTANCE: f32 = 8.0;
const MAX_WALL_DISTANCE: f32 = 40.0;
const WALL_STEP: f32 = 0.05;
const TINTS: [(u16, u32); 5] = [
    (METAL, 0x00a0_a8b0),
    (BRICK, 0x0090_4030),
    (ROCK, 0x0070_7068),
    (FLOOR, 0x0060_6060),
    (LIGHTPANEL, 0x00e0_e8f0),
];

/// The camera and the thing it looks at.
struct Stage {
    view: View,
    wall: Vec3,
    normal: Vec3,
}

/// Faces the first wall along the start's view, from `VIEW_DISTANCE` away.
fn stage(r: &LevelRenderer, start: Vec3, yaw: f32) -> Stage {
    let eye = View::from_angles(start, yaw, 0.0, 0.0);
    let forward = eye.orientation.rotate(Vec3::new(0.0, 0.0, -1.0));
    let steps = (MAX_WALL_DISTANCE / WALL_STEP) as u32;
    let hit = (0..steps)
        .map(|i| start + forward * (i as f32 * WALL_STEP))
        .find(|p| r.scene().bsp().is_solid(p.to_f64()))
        .unwrap_or(start + forward * MAX_WALL_DISTANCE);
    let back = (hit - start).length().min(VIEW_DISTANCE);
    Stage {
        view: View {
            pos: hit - forward * back,
            ..eye
        },
        wall: hit - forward * WALL_STEP,
        normal: forward * -1.0,
    }
}

/// One effect's first frame.
type Fire<'a> = &'a dyn Fn(&mut CombatFx, &LevelRenderer, &Stage);

fn wall_shot(material: u16, tint: u32) -> impl Fn(&mut CombatFx, &LevelRenderer, &Stage) {
    move |fx, _, s| {
        fx.shot(&ShotFx {
            by_player: true,
            muzzle: s.view.pos + s.view.orientation.rotate(Vec3::new(0.22, -0.18, -0.35)),
            end: s.wall,
            normal: s.normal,
            impact: Impact::Surface { material, tint },
        });
    }
}

fn shatter(material: u16, tint: u32) -> impl Fn(&mut CombatFx, &LevelRenderer, &Stage) {
    move |fx, _, s| {
        let at = s.wall + s.normal * 3.0;
        let half = Vec3::new(1.5, 1.5, 1.5);
        let bounds = Aabb {
            min: at - half,
            max: at + half,
        };
        fx.shatter(bounds, material, tint);
    }
}

/// The frame with effects drawn, and how long the renderer took over it.
fn draw(
    r: &LevelRenderer,
    fx: &mut CombatFx,
    s: &Stage,
    draws: &mut mscent_render::particles::ParticleDraws,
    bank: &SpriteBank,
    fb: &mut Framebuffer,
) -> f64 {
    let kick: ViewKick = fx.view_kick(r.scene(), s.view.pos);
    let view = View {
        pos: s.view.pos + kick.shake,
        ..s.view
    };
    if let Some(cam) = r.camera(&view, fb.width(), fb.height()) {
        fx.collect(&cam, r.fog(), None, draws);
    }
    let effects = Effects {
        particles: Some((&*draws, bank)),
        lights: fx.lights(r.scene()),
    };
    let start = Instant::now();
    r.render_with_effects(fb, &view, Quality::High, &[], None, &effects);
    let ms = start.elapsed().as_secs_f64() * 1000.0;
    if kick.white > 0 {
        simd::lerp_toward(fb.pixels_mut(), 0xffff_ffff, kick.white);
    }
    ms
}

fn blit(sheet: &mut [u32], fb: &Framebuffer, cell: usize) {
    let (w, h) = (W as usize, H as usize);
    let (cw, ch) = (w / 2, h / 2);
    for y in 0..ch {
        for x in 0..cw {
            let px = fb.pixels()[(y * 2) * w + x * 2];
            sheet[y * (cw * TIMES.len()) + cell * cw + x] = px;
        }
    }
}

fn run(name: &str, fire: Fire, r: &LevelRenderer, stage: &Stage, baseline: f64) {
    let (bank, mut draws) = (SpriteBank::new(), CombatFx::new_draws());
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let mut fx = CombatFx::new();
    fire(&mut fx, r, stage);
    let (cw, ch) = (W as usize / 2, H as usize / 2);
    let mut sheet = vec![0xff00_0000; cw * TIMES.len() * ch];
    let last = (TIMES[TIMES.len() - 1] * 60.0) as u32 + 1;
    let (mut peak_ms, mut peak_units, mut next, mut peak_at) = (0.0f64, 0.0f32, 0, 0);
    for frame in 0..=last {
        let ms = draw(r, &mut fx, stage, &mut draws, &bank, &mut fb);
        let units = cost_of(&draws, None, W as f32, H as f32);
        if ms > peak_ms {
            (peak_ms, peak_at) = (ms, frame);
        }
        peak_units = peak_units.max(units);
        if next < TIMES.len() && frame as f32 / 60.0 >= TIMES[next] {
            blit(&mut sheet, &fb, next);
            next += 1;
        }
        fx.advance(FRAME_NS, r.scene().bsp());
    }
    let png = encode_png((cw * TIMES.len()) as u32, ch as u32, &sheet).expect("a sheet");
    std::fs::write(format!("target/shots/fx/{name}.png"), png).expect("written");
    println!(
        "{name:<16} peak {peak_ms:>6.2} ms at frame {peak_at:>3} ({:>+6.2} over the empty frame)  \
         {:>4.2} budget screens",
        peak_ms - baseline,
        peak_units / (W * H) as f32,
    );
}

fn main() {
    std::fs::create_dir_all("target/shots/fx").expect("a folder");
    let materials = assets::load_materials(None).expect("textures");
    let level = showcase_level();
    let arena = mscent_game::Arena::from_level(&level).expect("a level");
    let cfg = mscent_render::BuildConfig::default();
    let r = LevelRenderer::from_level(&level, materials, &cfg).expect("a renderer");
    let stage = stage(&r, arena.player_start, arena.player_yaw);
    let (bank, mut draws) = (SpriteBank::new(), CombatFx::new_draws());
    let mut fb = Framebuffer::new(W, H).expect("a target");
    let mut empty = CombatFx::new();
    let baseline = (0..16)
        .map(|_| draw(&r, &mut empty, &stage, &mut draws, &bank, &mut fb))
        .fold(f64::MAX, f64::min);
    println!("empty frame {baseline:.2} ms on the default workers, 640x360, High");
    for (material, tint) in TINTS {
        run(
            &format!("hit_{material}"),
            &wall_shot(material, tint),
            &r,
            &stage,
            baseline,
        );
        run(
            &format!("shatter_{material}"),
            &shatter(material, tint),
            &r,
            &stage,
            baseline,
        );
    }
    let drone = |fx: &mut CombatFx, _: &LevelRenderer, s: &Stage| {
        let at = s.wall + s.normal * 3.0;
        fx.shot(&ShotFx {
            by_player: true,
            muzzle: s.view.pos,
            end: at,
            normal: s.normal,
            impact: Impact::Drone,
        });
    };
    let explosion = |fx: &mut CombatFx, _: &LevelRenderer, s: &Stage| {
        fx.drone_explosion(s.wall + s.normal * 3.0, DRONE_TINT);
    };
    let blast = |fx: &mut CombatFx, r: &LevelRenderer, s: &Stage| {
        fx.blast(s.wall + s.normal * 3.0, 12.0, r.scene().bsp());
    };
    run("drone_hit", &drone, &r, &stage, baseline);
    run("drone_explosion", &explosion, &r, &stage, baseline);
    run("blast", &blast, &r, &stage, baseline);
}
