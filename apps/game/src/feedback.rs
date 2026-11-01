//! Enemy attack feedback: a tracer from an enemy that fired, a muzzle flash
//! on it, and an edge marker pointing at whoever hurt the player. Without
//! these a hitscan shot from across the room is invisible damage.

use mscent_game::{Event as GameEvent, Game};
use mscent_math::Vec3;
use mscent_render::{Camera, Framebuffer, View, fill_rect};

/// Frames a tracer or damage marker stays up.
const TRACER_FRAMES: u32 = 10;
const HURT_FRAMES: u32 = 24;
const TRACER_COLOUR: u32 = 0xffff_6a30;
const FLASH_COLOUR: u32 = 0xffff_f0a0;
const MARKER_COLOUR: u32 = 0xffff_2020;

#[derive(Clone, Debug, Default)]
pub struct Feedback {
    /// World positions of enemies that just fired, with frames left.
    tracers: Vec<(Vec3, u32)>,
    /// Where the last damage came from, with frames left.
    hurt_from: Option<(Vec3, u32)>,
    /// The attacker of the damage event that follows, set by `EnemyFired`.
    last_shooter: Option<Vec3>,
}

impl Feedback {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Records the game events of a step.
    pub fn note(&mut self, game: &Game, event: &GameEvent) {
        match *event {
            GameEvent::EnemyFired(i) => {
                if let Some(e) = game.enemies.get(i) {
                    self.tracers.push((e.pos(), TRACER_FRAMES));
                    self.last_shooter = Some(e.pos());
                }
            }
            GameEvent::PlayerHurt(_) => {
                if let Some(p) = self.last_shooter {
                    self.hurt_from = Some((p, HURT_FRAMES));
                }
            }
            _ => {}
        }
    }

    /// Draws this frame's marks and ages them.
    pub fn draw(&mut self, fb: &mut Framebuffer, view: &View, fov_y: f32) {
        let Some(cam) = view.camera(fov_y, 0.1, 400.0, fb.width(), fb.height()) else {
            return;
        };
        let (w, h) = (fb.width() as f32, fb.height() as f32);
        // The muzzle of the attacker is drawn at the screen point the shot
        // converges on: just below the crosshair, where the player is.
        let target = (w * 0.5, h * 0.62);
        for &(pos, left) in &self.tracers {
            let Some(p) = cam.project(cam.world_to_camera(pos)) else {
                continue;
            };
            let fade = left as f32 / TRACER_FRAMES as f32;
            line(fb, (p.x, p.y), target, TRACER_COLOUR, fade);
            let r = (2.0 + 6.0 * fade) as i32;
            fill_rect(
                fb,
                p.x as i32 - r,
                p.y as i32 - r,
                (2 * r) as u32,
                (2 * r) as u32,
                FLASH_COLOUR,
            );
        }
        if let Some((pos, _)) = self.hurt_from {
            marker(fb, &cam, pos);
        }
        self.tracers.retain_mut(|t| {
            t.1 = t.1.saturating_sub(1);
            t.1 > 0
        });
        self.hurt_from = self
            .hurt_from
            .and_then(|(p, n)| (n > 1).then_some((p, n - 1)));
    }
}

/// Dotted line of small squares; `fade` in 0..=1 thins it out.
fn line(fb: &mut Framebuffer, a: (f32, f32), b: (f32, f32), colour: u32, fade: f32) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).min(2000.0);
    let n = (len / 3.0) as i32;
    let keep = (fade * 3.0).ceil().max(1.0) as i32;
    for i in 0..n {
        if i % 3 >= keep {
            continue;
        }
        let t = i as f32 / n.max(1) as f32;
        fill_rect(
            fb,
            (a.0 + dx * t) as i32,
            (a.1 + dy * t) as i32,
            3,
            3,
            colour,
        );
    }
}

/// A red bar on the screen edge toward the attacker, or on the attacker
/// itself's side when it is in front but off screen.
fn marker(fb: &mut Framebuffer, cam: &Camera, pos: Vec3) {
    let c = cam.world_to_camera(pos);
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    let ang = c.y.atan2(c.x);
    let (cx, cy) = (w / 2, h / 2);
    let (ex, ey) = (ang.cos() * cx as f32 * 0.9, -ang.sin() * cy as f32 * 0.9);
    let (mx, my) = (cx + ex as i32, cy + ey as i32);
    fill_rect(fb, mx - 10, my - 10, 20, 20, MARKER_COLOUR);
}
