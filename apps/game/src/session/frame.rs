//! Making a picture of the session: the camera for this frame, then the world,
//! its effects and what goes over them.

use mscent_collide::STEP_HZ;
use mscent_game::Screen;
use mscent_game::fx::combat::ViewKick;
use mscent_game::pilot::{EYE_RISE, heading_of};
use mscent_math::{Quat, Vec3};
use mscent_render::model::{ModelInstance, ModelTable};

use crate::drawn::enemy_models;
use mscent_render::particles::{ParticleDraws, SpriteBank};
use mscent_render::{
    Actor, DynLight, Effects, FrameInput, Framebuffer, LevelAssets, LevelRenderer, MoverPose,
    Quality, View, layer_over, simd,
};

use super::GameSession;
use crate::drawn::{actors_of, mover_poses_of};

const WHITE: u32 = 0xffff_ffff;
/// The particle budget's depth is a quarter of its frame each way, and at most
/// this wide: it probes large sprites at five points, which a coarse depth
/// answers, and it is drawn on the CPU each frame. The showcase's faces drawn
/// flat cost 0.16 ms at 160x90, 0.52 at 320x180 and 4.0 at 960x540.
const ESTIMATE_SHARE: u32 = 4;
const ESTIMATE_WIDTH: u32 = 160;
/// How far behind and above the played body a chase view stands: for the stock
/// ship (a bigger one pushes it back as far as it is bigger) and for the pilot.
const CHASE_BACK: f32 = 3.0;
const CHASE_UP: f32 = 0.8;

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
    /// Where the camera is, before anything cosmetic moves it: the pilot's
    /// eyes, or the ship's centre bobbing as it hovers.
    fn eye(&self) -> Vec3 {
        let alpha = self.stepper.alpha();
        if let Some(p) = &self.game.pilot {
            return p.body.prev_pos.lerp(p.body.pos, alpha) + Vec3::new(0.0, 0.0, EYE_RISE);
        }
        let p = &self.game.player;
        p.body.prev_pos.lerp(p.body.pos, alpha) + p.bob_offset()
    }

    /// The camera for this frame, interpolated between the last two steps.
    pub fn view(&self) -> View {
        let alpha = self.stepper.alpha();
        let turn = nlerp(self.prev_orientation, self.game.view_orientation(), alpha);
        let (orientation, reach) = match self.game.pilot {
            Some(_) => (turn, 1.0),
            None => {
                let ship = &self.game.player;
                ((turn * ship.bank_rotation()).normalized(), ship.scale)
            }
        };
        let mut pos = self.eye() + self.kick.shake;
        if self.chase {
            let (back, up) = (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0));
            pos = pos
                + orientation.rotate(back) * (CHASE_BACK * reach)
                + orientation.rotate(up) * (CHASE_UP * reach);
        }
        View { pos, orientation }
    }

    /// The ship and the pilot this frame: first what every renderer draws (the
    /// ship parked while the pilot walks), then the body being played, which
    /// only the hardware renderer draws (in a chase view) or shadows.
    fn crew_now(&self, view: &View) -> (Option<ModelInstance>, Option<ModelInstance>) {
        let Some(crew) = &self.crew else {
            return (None, None);
        };
        let ship = &self.game.player;
        let Some(pilot) = &self.game.pilot else {
            let pos = ship.body.prev_pos.lerp(ship.body.pos, self.stepper.alpha());
            let flying = crew.ship(pos, view.orientation, ship.scale, self.chase);
            return (None, Some(flying));
        };
        let parked = crew.ship(ship.pos(), ship.orientation, ship.scale, true);
        let feet = pilot
            .body
            .prev_pos
            .lerp(pilot.body.pos, self.stepper.alpha());
        let walking = crew.pilot(feet, heading_of(view.orientation), self.chase);
        (Some(parked), Some(walking))
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
        let at = self.prepare((width, height));
        let effects = Effects {
            particles: Some((&self.draws, &self.bank)),
            lights: self.fx.lights(self.renderer.scene()),
            poses: &at.poses,
            vista_time: at.vista_time,
            models: models_of(&self.renderer, &at),
        };
        let input = FrameInput {
            view: &at.view,
            quality: self.quality,
            actors: &at.actors,
            effects,
            // The particle budget reads its own estimate, so nothing reads it.
            wants_depth: false,
        };
        let drawn = if other(self.renderer.assets(), &mut fb, &input) {
            Some(())
        } else {
            self.renderer
                .draw_frame(&mut fb, &software_input(&input, at.software))
                .map(|_| ())
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
    /// the world to draw at `world` pixels, the layer of `layer` pixels the HUD,
    /// feedback, menus and notices are drawn into, and the white flash to put
    /// between them, and says whether it showed them. Where it did not, software
    /// draws the world at the layer's size and the layer goes over it, and that
    /// frame is returned to be presented.
    pub fn render_layered(
        &mut self,
        (world, layer_size): ((u32, u32), (u32, u32)),
        show: &mut dyn FnMut(&LevelAssets, &FrameInput, &Framebuffer, u32) -> bool,
    ) -> Option<Option<&Framebuffer>> {
        let mut layer = take_sized(&mut self.layer, layer_size)?;
        layer.clear(0);
        let at = self.prepare(world);
        self.draw_over(&mut layer, &at.view);
        let lights = self.fx.lights(self.renderer.scene());
        let models = models_of(&self.renderer, &at);
        let input = frame_input(
            &at,
            (&self.draws, &self.bank),
            (lights, models),
            self.quality,
        );
        let shown = show(self.renderer.assets(), &input, &layer, self.kick.white);
        let mut fb = None;
        if !shown {
            // The particles were placed on a screen of the world's size.
            let at = self.prepare(layer_size);
            let lights = self.fx.lights(self.renderer.scene());
            let models = models_of(&self.renderer, &at);
            let input = frame_input(
                &at,
                (&self.draws, &self.bank),
                (lights, models),
                self.quality,
            );
            let mut frame = take_sized(&mut self.fb, layer_size)?;
            if self
                .renderer
                .draw_frame(&mut frame, &software_input(&input, at.software))
                .is_some()
            {
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
    /// particles, thinned by what the estimate of its depth hides.
    fn prepare(&mut self, (width, height): (u32, u32)) -> Prepared {
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
        let estimate = self.estimate(&view, (width, height));
        match self.renderer.camera(&view, width, height) {
            Some(cam) => {
                let fog = self.renderer.fog();
                self.fx
                    .collect(&cam, fog, estimate.as_ref(), &mut self.draws);
            }
            None => self.draws.clear(),
        }
        self.estimate = estimate;
        let mut models = enemy_models(&self.game, alpha, self.renderer.assets());
        let (shared, played) = self.crew_now(&view);
        models.extend(shared);
        let software = models.len();
        models.extend(played);
        Prepared {
            view,
            actors: actors_of(&self.game),
            poses: mover_poses_of(&self.game, alpha),
            vista_time: self.vista_time(),
            models,
            software,
        }
    }

    /// The depth the particle budget reads for a frame of `size` from `view`: its
    /// visible faces, flat, at a fraction of its size each way. Drawn on the CPU
    /// whatever draws the frame, so every renderer thins the same particles, and
    /// one that reads nothing back has it too. Faces only: what an actor or a
    /// mover hides is not counted, which only costs a sprite behind one.
    fn estimate(&mut self, view: &View, (width, height): (u32, u32)) -> Option<Framebuffer> {
        let share = width.div_ceil(ESTIMATE_WIDTH).max(ESTIMATE_SHARE);
        let size = ((width / share).max(16), (height / share).max(16));
        let cam = self.renderer.camera(view, size.0, size.1)?;
        let mut fb = take_sized(&mut self.estimate, size)?;
        fb.clear(0);
        self.renderer.scene().render(&mut fb, &cam, None);
        Some(fb)
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
    /// The enemies, the parked ship if the pilot is out, then the body being
    /// played, if the session has their models.
    models: Vec<ModelInstance>,
    /// How many of `models` every renderer draws: all but the played body.
    software: usize,
}

/// The framebuffer in `slot`, made again at `size` if it is not that size.
/// `None` for a size a framebuffer cannot have.
fn take_sized(slot: &mut Option<Framebuffer>, (width, height): (u32, u32)) -> Option<Framebuffer> {
    match slot.take() {
        Some(fb) if (fb.width(), fb.height()) == (width, height) => Some(fb),
        _ => Framebuffer::new(width, height),
    }
}

/// `input` as software draws it: its first `shared` models, without the played
/// body after them, which only the hardware renderer draws (and shadows), so
/// that a chase view's body is not drawn by whichever renderer happens to be in
/// use. The parked ship is among the shared ones.
fn software_input<'a>(input: &FrameInput<'a>, shared: usize) -> FrameInput<'a> {
    let models = input.effects.models;
    FrameInput {
        view: input.view,
        quality: input.quality,
        actors: input.actors,
        effects: Effects {
            models: models.map(|(table, all)| (table, &all[..shared.min(all.len())])),
            ..input.effects
        },
        wants_depth: input.wants_depth,
    }
}

/// What a frame draws models with: the level's table, and the enemies, the
/// ship and the pilot where `at` has them.
fn models_of<'a>(
    renderer: &'a LevelRenderer,
    at: &'a Prepared,
) -> Option<(&'a ModelTable, &'a [ModelInstance])> {
    Some((renderer.assets().models(), &at.models))
}

/// The world of a frame `at` describes, with nothing read back after it.
fn frame_input<'a>(
    at: &'a Prepared,
    particles: (&'a ParticleDraws, &'a SpriteBank),
    (lights, models): (
        &'a [DynLight],
        Option<(&'a ModelTable, &'a [ModelInstance])>,
    ),
    quality: Quality,
) -> FrameInput<'a> {
    FrameInput {
        view: &at.view,
        quality,
        actors: &at.actors,
        effects: Effects {
            particles: Some(particles),
            lights,
            poses: &at.poses,
            vista_time: at.vista_time,
            models,
        },
        wants_depth: false,
    }
}
