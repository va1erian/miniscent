//! [`GameSession`]: one playable level with no window attached.
//!
//! The session owns the simulation ([`Game`]), the compiled and baked
//! renderer, the menu state and the device state. A host feeds it elapsed
//! time and platform events through [`GameSession::step`] and asks for a
//! picture with [`GameSession::render`]. The desktop app, the headless
//! screenshot runs and the editor's Instant Play all drive it this way.

use std::time::Duration;

use mscent_collide::FixedStepper;
use mscent_game::{
    Arena, Event as GameEvent, Game, HudModel, LoadError, Menu, MenuAction, MenuCommand, Phase,
    Screen, draw_hud, draw_menu,
};
use mscent_math::{Quat, Vec3};
use mscent_platform::{Event, Key, MouseButton};
use mscent_render::{
    Actor, ActorKind, BakeCache, BuildConfig, BuildError, Framebuffer, LevelRenderer,
    MaterialTable, MoverPose, Quality, View, draw_text, fill_rect,
};
use mscent_world::Level;

use crate::controls::Controls;
use crate::feedback::Feedback;
use mscent_script::{ScriptError, Source};

/// Radians of turn per pixel of mouse movement.
const DEFAULT_SENSITIVITY: f32 = 0.0022;
/// Frames a muzzle flash or hit marker stays on screen.
const FLASH_FRAMES: u32 = 6;

/// How a session is built and drawn.
#[derive(Clone, Copy, Debug)]
pub struct SessionConfig {
    pub quality: Quality,
    pub build: BuildConfig,
    /// Radians per mouse pixel.
    pub mouse_sensitivity: f32,
    /// A script comes with this level, so it may have no goal of its own.
    pub scripted: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            quality: Quality::High,
            build: BuildConfig::default(),
            mouse_sensitivity: DEFAULT_SENSITIVITY,
            scripted: false,
        }
    }
}

/// Why a level cannot be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    Load(LoadError),
    Build(BuildError),
}

impl core::fmt::Display for SessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Load(e) => write!(f, "level cannot be played: {e:?}"),
            Self::Build(e) => write!(f, "level cannot be built: {e:?}"),
        }
    }
}

/// What the host should do after a [`GameSession::step`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepOutcome {
    /// The player chose to quit (Esc on the pause menu, or a menu item).
    pub quit: bool,
    /// Whether the mouse should be captured: true exactly while playing.
    pub capture_mouse: bool,
}

/// Measurements the host reports for the HUD.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub fps: f32,
    pub frame_ms: f32,
}

pub struct GameSession {
    game: Game,
    renderer: LevelRenderer,
    menu: Menu,
    controls: Controls,
    stepper: FixedStepper,
    prev_orientation: Quat,
    quality: Quality,
    sensitivity: f32,
    stats: FrameStats,
    fb: Option<Framebuffer>,
    muzzle: u32,
    hit: u32,
    kill: u32,
    feedback: Feedback,
}

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

/// The game library keeps the camera frame (view along -Z); the level is
/// Z-up, so a spawn yaw is turned into the matching camera orientation.
fn spawn_orientation(yaw: f32) -> Quat {
    View::from_angles(Vec3::ZERO, yaw, 0.0, 0.0).orientation
}

impl GameSession {
    /// Compiles, bakes and starts `level`. The first menu ("Start") is
    /// skipped: a session begins in play.
    pub fn new(
        level: &Level,
        materials: MaterialTable,
        cfg: &SessionConfig,
    ) -> Result<Self, SessionError> {
        Self::new_cached(level, materials, cfg, &mut BakeCache::new())
    }

    /// Like [`GameSession::new`], with the lightmap bake going through
    /// `cache`: a level whose geometry and lights did not change since the
    /// cache's last bake (a new texture mapping, say) is not baked again.
    pub fn new_cached(
        level: &Level,
        materials: MaterialTable,
        cfg: &SessionConfig,
        cache: &mut BakeCache,
    ) -> Result<Self, SessionError> {
        let arena = if cfg.scripted {
            Arena::from_scripted_level(level)
        } else {
            Arena::from_level(level)
        }
        .map_err(SessionError::Load)?;
        let renderer = LevelRenderer::from_level_cached(level, materials, &cfg.build, cache)
            .map_err(SessionError::Build)?;
        Ok(Self::with_renderer(arena, renderer, cfg))
    }

    /// Starts a level that is already compiled and lit (the editor prepares
    /// its renderer in stages). The first menu is skipped, as in
    /// [`GameSession::new`].
    pub fn with_renderer(arena: Arena, renderer: LevelRenderer, cfg: &SessionConfig) -> Self {
        let mut game = Game::new(arena);
        game.player.orientation = spawn_orientation(game.arena.player_yaw);
        game.player.up_ref = Some(Vec3::new(0.0, 0.0, 1.0));
        Self::with_game(game, renderer, cfg)
    }

    /// Wraps a game already under way, as a test does to take a picture of it.
    pub fn with_game(game: Game, renderer: LevelRenderer, cfg: &SessionConfig) -> Self {
        let mut menu = Menu::new();
        // Item 0 of the main menu is "Start".
        menu.press(MenuCommand::Select);
        Self {
            prev_orientation: game.player.orientation,
            game,
            renderer,
            menu,
            controls: Controls::default(),
            stepper: FixedStepper::new(),
            quality: cfg.quality,
            sensitivity: cfg.mouse_sensitivity,
            stats: FrameStats::default(),
            fb: None,
            muzzle: 0,
            hit: 0,
            kill: 0,
            feedback: Feedback::default(),
        }
    }

    /// Replaces what the level is drawn with and keeps the running game: new
    /// lighting for the same level arrives while it is being played.
    pub fn set_renderer(&mut self, renderer: LevelRenderer) {
        self.renderer = renderer;
    }

    /// The game and the renderer, taken apart again.
    pub fn into_parts(self) -> (Game, LevelRenderer) {
        (self.game, self.renderer)
    }

    /// The renderer, once the game is over with.
    pub fn into_renderer(self) -> LevelRenderer {
        self.renderer
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn phase(&self) -> Phase {
        self.game.phase
    }

    pub fn screen(&self) -> Screen {
        self.menu.screen()
    }

    pub fn renderer(&self) -> &LevelRenderer {
        &self.renderer
    }

    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
    }

    pub fn set_stats(&mut self, stats: FrameStats) {
        self.stats = stats;
    }

    /// Compiles a level script and starts the level over on it. Returns the
    /// compiler's warnings, or every error with nothing changed.
    pub fn load_script(
        &mut self,
        sources: &[Source],
    ) -> Result<Vec<ScriptError>, Vec<ScriptError>> {
        let warnings = self.game.load_script(sources)?;
        self.restart();
        Ok(warnings)
    }

    /// Starts the level over: player, enemies, weapon and menu.
    pub fn restart(&mut self) {
        self.game.restart();
        self.game.player.up_ref = Some(Vec3::new(0.0, 0.0, 1.0));
        self.game.player.orientation = spawn_orientation(self.game.arena.player_yaw);
        self.prev_orientation = self.game.player.orientation;
        self.stepper = FixedStepper::new();
        self.controls.release_all();
        self.menu = Menu::new();
        self.menu.press(MenuCommand::Select);
        (self.muzzle, self.hit, self.kill) = (0, 0, 0);
        self.feedback.clear();
    }

    fn playing(&self) -> bool {
        self.menu.screen() == Screen::Hidden && self.game.phase == Phase::Playing
    }

    /// Advances the session by `dt` of wall-clock time, applying `events`
    /// first. While a menu is up the simulation is frozen.
    pub fn step(&mut self, dt: Duration, events: &[Event]) -> StepOutcome {
        let mut quit = false;
        for e in events {
            quit |= self.on_event(e);
        }
        if self.playing() {
            self.look();
            let steps = self
                .stepper
                .advance(u64::try_from(dt.as_nanos()).unwrap_or(u64::MAX));
            let input = self.controls.input();
            for _ in 0..steps {
                self.prev_orientation = self.game.player.orientation;
                self.game.step(&input);
            }
            self.note_events();
            self.report_script();
            self.menu.sync_phase(self.game.phase);
        } else {
            self.controls.take_mouse();
        }
        StepOutcome {
            quit,
            capture_mouse: self.playing(),
        }
    }

    /// Until the HUD shows these (M9.8), the console does.
    fn report_script(&mut self) {
        for message in self.game.take_messages() {
            eprintln!("script says: {message}");
        }
        for error in self.game.take_script_errors() {
            eprintln!("script error: {error}");
        }
    }

    /// Returns true when the event asks to quit.
    fn on_event(&mut self, e: &Event) -> bool {
        // A click that resumes from pause is consumed: it must not also fire.
        if matches!(
            *e,
            Event::MouseButton {
                button: MouseButton::Left,
                pressed: true
            }
        ) && self.menu.screen() == Screen::Paused
        {
            self.menu.press(MenuCommand::Back);
            return false;
        }
        match *e {
            Event::Quit => return true,
            Event::FocusLost if self.playing() => {
                self.menu.press(MenuCommand::Back);
            }
            _ => {}
        }
        match self.controls.event(e) {
            Some(key) => self.on_press(key),
            None => false,
        }
    }

    fn on_press(&mut self, key: Key) -> bool {
        let command = match key {
            Key::Escape => {
                if self.menu.screen() == Screen::Hidden {
                    self.menu.press(MenuCommand::Back);
                    return false;
                }
                return true;
            }
            // R is thrust-up while flying; it restarts only from a menu or
            // the win/lose screen, never mid-run.
            Key::Char('R') if !self.playing() => {
                self.restart();
                return false;
            }
            Key::Up | Key::Char('W') => MenuCommand::Up,
            Key::Down | Key::Char('S') => MenuCommand::Down,
            Key::Enter => MenuCommand::Select,
            _ => return false,
        };
        if self.menu.screen() == Screen::Hidden {
            return false;
        }
        match self.menu.press(command) {
            MenuAction::Restart => self.restart(),
            MenuAction::Quit => return true,
            MenuAction::None | MenuAction::Start | MenuAction::Resume => {}
        }
        false
    }

    /// Mouse look: an immediate rotation about the body axes, so it needs
    /// no inertia and works at any roll.
    fn look(&mut self) {
        let (dx, dy) = self.controls.take_mouse();
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        let before = self.game.player.orientation;
        self.game
            .player
            .look(-dy * self.sensitivity, -dx * self.sensitivity);
        // Mouse look is instant: shift the interpolation start by the same
        // rotation so it is never smoothed.
        let delta = before.conjugate() * self.game.player.orientation;
        self.prev_orientation = (self.prev_orientation * delta).normalized();
    }

    fn note_events(&mut self) {
        for e in self.game.take_events() {
            match e {
                GameEvent::PlayerFired => self.muzzle = FLASH_FRAMES,
                GameEvent::EnemyHit(_) => self.hit = FLASH_FRAMES,
                GameEvent::EnemyKilled(_) => self.kill = FLASH_FRAMES * 2,
                _ => {}
            }
            self.feedback.note(&self.game, &e);
        }
    }

    /// The camera for this frame, interpolated between the last two steps.
    pub fn view(&self) -> View {
        let alpha = self.stepper.alpha();
        let p = &self.game.player;
        View {
            pos: p.body.prev_pos.lerp(p.body.pos, alpha),
            orientation: nlerp(self.prev_orientation, p.orientation, alpha),
        }
    }

    fn actors(&self) -> Vec<Actor> {
        let alpha = self.stepper.alpha();
        let eye = self.game.player.pos();
        let spin = self.game.ticks as f32 * 0.02;
        let goals = self.game.arena.goals.iter().map(|g| Actor {
            kind: ActorKind::Goal,
            pos: g.pos,
            yaw: spin,
        });
        let enemies = self.game.enemies.iter().filter(|e| e.alive()).map(|e| {
            let pos = e.body.prev_pos.lerp(e.body.pos, alpha);
            let to = eye - pos;
            Actor {
                kind: ActorKind::Enemy,
                pos,
                yaw: to.y.atan2(to.x) - core::f32::consts::FRAC_PI_2,
            }
        });
        goals.chain(enemies).collect()
    }

    fn mover_poses(&self) -> Vec<MoverPose> {
        let alpha = self.stepper.alpha();
        let movers = self.game.movers.iter();
        movers
            .map(|m| {
                let pose = m.drawn_pose(alpha);
                MoverPose {
                    rot: pose.rot,
                    pivot: pose.pivot.to_f32(),
                    offset: pose.offset.to_f32(),
                }
            })
            .collect()
    }

    /// Draws the current state at `width` x `height`: world, HUD, feedback
    /// and menus. `None` when the size is zero or the pose is invalid.
    pub fn render(&mut self, width: u32, height: u32) -> Option<&Framebuffer> {
        if self.fb.as_ref().map(|f| (f.width(), f.height())) != Some((width, height)) {
            self.fb = Some(Framebuffer::new(width, height)?);
        }
        let view = self.view();
        let actors = self.actors();
        let poses = self.mover_poses();
        let mut fb = self.fb.take()?;
        let drawn = self
            .renderer
            .render_posed(&mut fb, &view, self.quality, &actors, None, &poses);
        if drawn.is_some() {
            let fov = self.renderer.config().fov_y;
            self.feedback.draw(&mut fb, &view, fov);
            self.draw_overlays(&mut fb);
        }
        self.muzzle = self.muzzle.saturating_sub(1);
        self.hit = self.hit.saturating_sub(1);
        self.kill = self.kill.saturating_sub(1);
        self.fb = Some(fb);
        drawn.and(self.fb.as_ref())
    }

    fn draw_overlays(&self, fb: &mut Framebuffer) {
        draw_hud(fb, &HudModel::from_game(&self.game));
        let (w, h) = (fb.width() as i32, fb.height() as i32);
        let stats = format!("{:.0} FPS {:.1} MS", self.stats.fps, self.stats.frame_ms);
        let size = mscent_render::text_size(&stats, 1);
        draw_text(fb, w - size.0 as i32 - 8, 8, &stats, 0xffff_ffff, 1);
        if self.muzzle > 0 {
            fill_rect(fb, w / 2 - 3, h - 34, 6, 18, 0xffff_e060);
            fill_rect(fb, w / 2 - 8, h - 28, 16, 6, 0xffff_a030);
        }
        let (cx, cy) = (w / 2, h / 2);
        if self.hit > 0 {
            fill_rect(fb, cx - 9, cy - 9, 4, 4, 0xffff_3030);
            fill_rect(fb, cx + 6, cy - 9, 4, 4, 0xffff_3030);
            fill_rect(fb, cx - 9, cy + 6, 4, 4, 0xffff_3030);
            fill_rect(fb, cx + 6, cy + 6, 4, 4, 0xffff_3030);
        }
        if self.kill > 0 {
            draw_text(fb, cx - 12, cy + 16, "KILL", 0xffff_d040, 1);
        }
        if self.menu.screen() == Screen::Won || self.menu.screen() == Screen::Lost {
            let hint = "R TO RESTART";
            let tw = mscent_render::text_size(hint, 1).0 as i32;
            draw_text(fb, (w - tw) / 2, h - 24, hint, 0xffc0_c0c0, 1);
        }
        draw_menu(fb, &self.menu);
    }
}
