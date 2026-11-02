//! Making a picture of the session: the camera for this frame, then the world,
//! its effects and what goes over them.

use mscent_collide::STEP_HZ;
use mscent_game::Screen;
use mscent_game::fx::combat::ViewKick;
use mscent_math::{Quat, Vec3};
use mscent_render::{Effects, Framebuffer, View, simd};

use super::GameSession;
use crate::drawn::{actors_of, mover_poses_of};

const WHITE: u32 = 0xffff_ffff;

/// Cheap interpolation between nearby orientations.
fn nlerp(a: Quat, b: Quat, t: f32) -> Quat {
    let sign = if a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w < 0.0 {
        -1.0
    } else {
        1.0
    };
    let mix = |p: f32, q: f32| p + (q * sign - p) * t;
    Quat {
        x: mix(a.x, b.x),
        y: mix(a.y, b.y),
        z: mix(a.z, b.z),
        w: mix(a.w, b.w),
    }
    .normalized()
}

impl GameSession {
    /// Where the camera is, before anything cosmetic moves it.
    fn eye(&self) -> Vec3 {
        let p = &self.game.player;
        p.body.prev_pos.lerp(p.body.pos, self.stepper.alpha()) + p.bob_offset()
    }

    /// The camera for this frame, interpolated between the last two steps.
    pub fn view(&self) -> View {
        let alpha = self.stepper.alpha();
        let p = &self.game.player;
        View {
            pos: self.eye() + self.kick.shake,
            orientation: (nlerp(self.prev_orientation, p.orientation, alpha) * p.bank_rotation())
                .normalized(),
        }
    }

    /// Seconds of play for what a vista does over time: the fixed steps simulated
    /// so far plus the part of the next one, never a wall clock, so a scripted
    /// shot is the same every run. It stands still while the game does.
    fn vista_time(&self) -> f32 {
        ((self.game.ticks as f64 + f64::from(self.stepper.alpha())) / STEP_HZ as f64) as f32
    }

    /// Draws the current state at `width` x `height`: world, HUD, feedback
    /// and menus. `None` when the size is zero or the pose is invalid.
    pub fn render(&mut self, width: u32, height: u32) -> Option<&Framebuffer> {
        if self.fb.as_ref().map(|f| (f.width(), f.height())) != Some((width, height)) {
            self.fb = Some(Framebuffer::new(width, height)?);
        }
        // The particle clock stops on the pause menu, and a flash must not stay
        // up behind it for as long as the pause lasts.
        self.kick = if self.menu.screen() == Screen::Paused {
            ViewKick::default()
        } else {
            self.fx.view_kick_in(
                self.renderer.scene(),
                self.eye(),
                self.game.arena.fx_world(),
            )
        };
        let view = self.view();
        let alpha = self.stepper.alpha();
        let actors = actors_of(&self.game, alpha);
        let poses = mover_poses_of(&self.game, alpha);
        let mut fb = self.fb.take()?;
        match self.renderer.camera(&view, width, height) {
            // `fb` still holds the last frame, whose depth is the best guess
            // at what this one hides.
            Some(cam) => {
                let fog = self.renderer.fog();
                self.fx.collect(&cam, fog, Some(&fb), &mut self.draws);
            }
            None => self.draws.clear(),
        }
        let vista_time = self.vista_time();
        let effects = Effects {
            particles: Some((&self.draws, &self.bank)),
            lights: self.fx.lights(self.renderer.scene()),
            poses: &poses,
            vista_time,
        };
        let drawn = self.renderer.render_with_effects(
            &mut fb,
            &view,
            self.quality,
            &actors,
            None,
            &effects,
        );
        if drawn.is_some() {
            if self.kick.white > 0 {
                simd::lerp_toward(fb.pixels_mut(), WHITE, self.kick.white);
            }
            let fov = self.renderer.config().fov_y;
            self.feedback.draw(&mut fb, &view, fov);
            self.draw_overlays(&mut fb);
            self.notices.draw(&mut fb);
        }
        self.hit = self.hit.saturating_sub(1);
        self.kill = self.kill.saturating_sub(1);
        self.fb = Some(fb);
        drawn.and(self.fb.as_ref())
    }
}
