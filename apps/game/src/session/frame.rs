//! Making a picture of the session: the camera for this frame, then the world,
//! its effects and what goes over them.

use mscent_collide::STEP_HZ;
use mscent_game::Screen;
use mscent_game::fx::combat::ViewKick;
use mscent_math::{Quat, Vec3};
use mscent_render::{
    Actor, Effects, FrameInput, FrameRenderer, Framebuffer, LevelAssets, MoverPose, View,
    layer_over, simd,
};

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
        self.render_through(width, height, &mut |_, _, _| false)
    }

    /// [`Self::render`], with the world drawn by `other` where it can, from the
    /// session's own level: another renderer, which says whether it drew the
    /// frame, its depth too, as the particle budget reads it. Where it does not,
    /// software draws it.
    pub fn render_through(
        &mut self,
        width: u32,
        height: u32,
        other: &mut dyn FnMut(&LevelAssets, &mut Framebuffer, &FrameInput) -> bool,
    ) -> Option<&Framebuffer> {
        let mut fb = take_sized(&mut self.fb, (width, height))?;
        // `fb` still holds the last frame, whose depth is the best guess at
        // what this one hides.
        let at = self.prepare((width, height), Some(&fb));
        let effects = Effects {
            particles: Some((&self.draws, &self.bank)),
            lights: self.fx.lights(self.renderer.scene()),
            poses: &at.poses,
            vista_time: at.vista_time,
            models: None,
        };
        let input = FrameInput {
            view: &at.view,
            quality: self.quality,
            actors: &at.actors,
            effects,
            // The particle budget reads it, next frame.
            wants_depth: true,
        };
        let drawn = if other(self.renderer.assets(), &mut fb, &input) {
            Some(())
        } else {
            self.renderer.draw(&mut fb, &input).map(|_| ())
        };
        if drawn.is_some() {
            if self.kick.white > 0 {
                simd::lerp_toward(fb.pixels_mut(), WHITE, self.kick.white);
            }
            self.draw_over(&mut fb, &at.view);
        }
        self.count_down();
        self.fb = Some(fb);
        drawn.and(self.fb.as_ref())
    }

    /// [`Self::render`] for a renderer that shows the frame itself: `show` gets
    /// the world to draw, the layer the HUD, feedback, menus and notices are
    /// drawn into, and the white flash to put between them, and says whether it
    /// showed them. Where it did not, software draws the world and the layer goes
    /// over it, and that frame is returned to be presented. The particle budget
    /// has no depth here: nothing is read back.
    pub fn render_layered(
        &mut self,
        (width, height): (u32, u32),
        show: &mut dyn FnMut(&LevelAssets, &FrameInput, &Framebuffer, u32) -> bool,
    ) -> Option<Option<&Framebuffer>> {
        let mut layer = take_sized(&mut self.layer, (width, height))?;
        layer.clear(0);
        let at = self.prepare((width, height), None);
        self.draw_over(&mut layer, &at.view);
        let effects = Effects {
            particles: Some((&self.draws, &self.bank)),
            lights: self.fx.lights(self.renderer.scene()),
            poses: &at.poses,
            vista_time: at.vista_time,
            models: None,
        };
        let input = FrameInput {
            view: &at.view,
            quality: self.quality,
            actors: &at.actors,
            effects,
            wants_depth: false,
        };
        let shown = show(self.renderer.assets(), &input, &layer, self.kick.white);
        let mut fb = None;
        if !shown {
            let mut frame = take_sized(&mut self.fb, (width, height))?;
            if self.renderer.draw(&mut frame, &input).is_some() {
                if self.kick.white > 0 {
                    simd::lerp_toward(frame.pixels_mut(), WHITE, self.kick.white);
                }
                layer_over(&mut frame, &layer);
            }
            fb = Some(frame);
        }
        self.count_down();
        self.layer = Some(layer);
        if let Some(frame) = fb {
            self.fb = Some(frame);
            return Some(self.fb.as_ref());
        }
        Some(None)
    }

    /// What a frame of `size` draws, before any of it is drawn: the view kick,
    /// the camera's pose, the actors and movers where they are, and the
    /// particles, thinned by what `depth` hides.
    fn prepare(&mut self, (width, height): (u32, u32), depth: Option<&Framebuffer>) -> Prepared {
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
        match self.renderer.camera(&view, width, height) {
            Some(cam) => {
                let fog = self.renderer.fog();
                self.fx.collect(&cam, fog, depth, &mut self.draws);
            }
            None => self.draws.clear(),
        }
        Prepared {
            view,
            actors: actors_of(&self.game, alpha),
            poses: mover_poses_of(&self.game, alpha),
            vista_time: self.vista_time(),
        }
    }

    /// What goes over the world, into `fb`: feedback, the HUD and menus, and
    /// notices.
    fn draw_over(&mut self, fb: &mut Framebuffer, view: &View) {
        let fov = self.renderer.config().fov_y;
        self.feedback.draw(fb, view, fov);
        self.draw_overlays(fb);
        self.notices.draw(fb);
    }

    /// One frame less of the hit and kill markers.
    fn count_down(&mut self) {
        self.hit = self.hit.saturating_sub(1);
        self.kill = self.kill.saturating_sub(1);
    }
}

/// What [`GameSession::prepare`] gives a frame.
struct Prepared {
    view: View,
    actors: Vec<Actor>,
    poses: Vec<MoverPose>,
    vista_time: f32,
}

/// The framebuffer in `slot`, made again at `size` if it is not that size.
/// `None` for a size a framebuffer cannot have.
fn take_sized(slot: &mut Option<Framebuffer>, (width, height): (u32, u32)) -> Option<Framebuffer> {
    match slot.take() {
        Some(fb) if (fb.width(), fb.height()) == (width, height) => Some(fb),
        _ => Framebuffer::new(width, height),
    }
}
