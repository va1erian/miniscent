//! An edge marker pointing at whoever hurt the player, for an attacker out of
//! view. The shots themselves are world-space particles (`mscent_game::fx`).

use mscent_game::{Event as GameEvent, Game};
use mscent_math::Vec3;
use mscent_render::{Camera, Framebuffer, View, fill_rect};

/// Frames a damage marker stays up.
const HURT_FRAMES: u32 = 24;
const MARKER_COLOUR: u32 = 0xffff_2020;

#[derive(Clone, Debug, Default)]
pub struct Feedback {
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
        if let Some((pos, _)) = self.hurt_from {
            marker(fb, &cam, pos);
        }
        self.hurt_from = self
            .hurt_from
            .and_then(|(p, n)| (n > 1).then_some((p, n - 1)));
    }
}

/// A red block on the screen edge toward an attacker that is not in view.
fn marker(fb: &mut Framebuffer, cam: &Camera, pos: Vec3) {
    let c = cam.world_to_camera(pos);
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    let on_screen = cam
        .project(c)
        .is_some_and(|p| p.x >= 0.0 && p.y >= 0.0 && p.x < w as f32 && p.y < h as f32);
    if on_screen {
        return;
    }
    let ang = c.y.atan2(c.x);
    let (cx, cy) = (w / 2, h / 2);
    let (ex, ey) = (ang.cos() * cx as f32 * 0.9, -ang.sin() * cy as f32 * 0.9);
    let (mx, my) = (cx + ex as i32, cy + ey as i32);
    fill_rect(fb, mx - 10, my - 10, 20, 20, MARKER_COLOUR);
}
