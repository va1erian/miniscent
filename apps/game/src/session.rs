//! [`GameSession`]: one playable level with no window attached.
//!
//! The session owns the simulation ([`Game`]), the compiled and baked
//! renderer, the menu state and the device state. A host feeds it elapsed
//! time and platform events through [`GameSession::step`] and asks for a
//! picture with [`GameSession::render`]. The desktop app, the headless
//! screenshot runs and the editor's Instant Play all drive it this way.

use std::time::Duration;

use mscent_collide::FixedStepper;
use mscent_game::destruct::VoxelUpdate;
use mscent_game::fx::combat::{CombatFx, DRONE_TINT, ViewKick};
use mscent_game::{
    Arena, Event as GameEvent, Game, LoadError, Menu, MenuAction, MenuCommand, Phase, Screen,
};
use mscent_math::{Quat, Vec3};
use mscent_platform::{Event, Key, MouseButton};
use mscent_render::particles::{ParticleDraws, SpriteBank};
use mscent_render::{
    BakeCache, BuildConfig, BuildError, Framebuffer, LevelRenderer, MaterialTable, Quality, View,
    VoxelChange,
};
use mscent_world::Level;

use crate::controls::Controls;
use crate::feedback::Feedback;
use crate::notices::Notices;
use mscent_script::{ScriptError, Source};

mod frame;
mod overlay;
mod shots;

/// Radians of turn per pixel of mouse movement.
const DEFAULT_SENSITIVITY: f32 = 0.0022;
/// Frames a hit marker stays on screen.
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
    /// What goes over the world, for a renderer that shows the frame itself.
    layer: Option<Framebuffer>,
    hit: u32,
    kill: u32,
    feedback: Feedback,
    fx: CombatFx,
    draws: ParticleDraws,
    bank: SpriteBank,
    kick: ViewKick,
    notices: Notices,
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
        let mut fx = CombatFx::new();
        fx.set_quality(cfg.quality);
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
            layer: None,
            hit: 0,
            kill: 0,
            feedback: Feedback::default(),
            fx,
            draws: CombatFx::new_draws(),
            bank: SpriteBank::new(),
            kick: ViewKick::default(),
            notices: Notices::default(),
        }
    }

    /// Replaces what the level is drawn with and keeps the running game: new
    /// lighting for the same level arrives while it is being played.
    pub fn set_renderer(&mut self, renderer: LevelRenderer) {
        self.renderer = renderer;
    }

    /// Sets off a blast. Nothing in the game does this yet; damage from one is
    /// a separate gameplay item, and the effect waits for it.
    pub fn blast(&mut self, pos: Vec3, radius: f32) {
        self.fx.blast(pos, radius, self.game.arena.fx_world());
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

    /// The particles the last frame drew, which the frame's depth thinned.
    pub fn particle_draws(&self) -> &ParticleDraws {
        &self.draws
    }

    pub fn renderer(&self) -> &LevelRenderer {
        &self.renderer
    }

    /// For changes under a level that stays the same, between frames.
    pub fn renderer_mut(&mut self) -> &mut LevelRenderer {
        &mut self.renderer
    }

    pub fn set_quality(&mut self, quality: Quality) {
        self.quality = quality;
        self.fx.set_quality(quality);
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
        self.reset_after_game_restart();
        Ok(warnings)
    }

    /// Starts the level over: player, enemies, weapon and menu.
    pub fn restart(&mut self) {
        self.game.restart();
        self.reset_after_game_restart();
    }

    fn reset_after_game_restart(&mut self) {
        self.game.player.up_ref = Some(Vec3::new(0.0, 0.0, 1.0));
        self.game.player.orientation = spawn_orientation(self.game.arena.player_yaw);
        self.prev_orientation = self.game.player.orientation;
        self.stepper = FixedStepper::new();
        self.controls.release_all();
        self.menu = Menu::new();
        self.menu.press(MenuCommand::Select);
        self.sync_voxels();
        (self.hit, self.kill) = (0, 0);
        self.feedback.clear();
        self.fx.clear();
        self.notices.clear();
    }

    /// Gives the renderer what collision already has, in the same frame.
    fn sync_voxels(&mut self) {
        for update in self.game.take_voxel_updates() {
            self.renderer.apply_voxels(match update {
                VoxelUpdate::Chunks(list) => VoxelChange::Chunks(list),
                VoxelUpdate::Reset(world) => VoxelChange::Restore(world),
            });
        }
    }

    /// Takes the shot-up rock of a run back to the level's, for a view of the
    /// level between runs.
    pub fn restore_voxels(&mut self) {
        self.game.restore_voxels();
        self.sync_voxels();
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
        // Before this step's shots, so a new bolt is drawn before it ages. The
        // clock runs past the end of a level so a fatal explosion finishes, and
        // stops only on the pause menu.
        if self.menu.screen() != Screen::Paused {
            let ns = u64::try_from(dt.as_nanos()).unwrap_or(u64::MAX);
            self.fx.advance(ns, self.game.arena.fx_world());
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
            self.sync_voxels();
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

    /// Tells the player a script could not be reloaded.
    pub fn show_script_problem(&mut self, problem: &str) {
        for line in problem.lines() {
            eprintln!("{line}");
            self.notices.error(line);
        }
    }

    /// On screen for the player, and on the console for whoever is scripting.
    fn report_script(&mut self) {
        for message in self.game.take_messages() {
            eprintln!("script says: {message}");
            self.notices.message(&message);
        }
        for error in self.game.take_script_errors() {
            eprintln!("script error: {error}");
            self.notices.error(&error.to_string());
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
                GameEvent::EnemyHit(_) => self.hit = FLASH_FRAMES,
                GameEvent::EnemyKilled(i) => {
                    self.kill = FLASH_FRAMES * 2;
                    if let Some(enemy) = self.game.enemies.get(i) {
                        self.fx.drone_explosion(enemy.pos(), DRONE_TINT);
                    }
                }
                GameEvent::Shot {
                    shooter,
                    origin,
                    end,
                    normal,
                    struck,
                } => self.shot_effect(shooter, origin, end, normal, struck),
                _ => {}
            }
            self.feedback.note(&self.game, &e);
        }
    }
}
