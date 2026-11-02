//! The real [`Scene`]: a built level, drawn as the editor's fly camera view
//! or as the running game.

use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Effects, FrameInput, Framebuffer, Quality, VoxelChange, draw_text};

use super::gpu_view::{self, GpuView};
use super::highlight;
use super::worker::{Request, Scene};
use crate::build::Built;

/// How often a running level's script is checked for changes, in game time.
const WATCH_EVERY: Duration = Duration::from_millis(250);

pub struct LevelScene {
    built: Built,
    fb: Option<Framebuffer>,
    gpu: GpuView,
    since_watch: Duration,
}

impl LevelScene {
    pub fn new(built: Built) -> LevelScene {
        LevelScene {
            built,
            fb: None,
            gpu: GpuView::default(),
            since_watch: Duration::ZERO,
        }
    }

    /// Runs the level on its script as saved now. A broken one leaves the old
    /// one running, and says so on the screen.
    fn reload_script(&mut self) {
        let Built { product, watch, .. } = &mut self.built;
        let (Some(watch), crate::build::Product::Playable(session)) = (watch, product) else {
            return;
        };
        if let Err(problem) = watch.reload_if_changed(session) {
            session.show_script_problem(&problem);
        }
    }

    /// Draws on the GPU whatever `gpu_view::set_wanted` says, for a test.
    #[cfg(test)]
    pub fn on_gpu(built: Built) -> LevelScene {
        let mut scene = LevelScene::new(built);
        scene.gpu.forced = true;
        scene
    }

    /// The running game, for tests that look inside.
    #[cfg(test)]
    pub fn session(&mut self) -> Option<&mut miniscent_game::GameSession> {
        self.built.session_mut()
    }
}

impl Scene for LevelScene {
    /// The reload comes last: a restart clears the screen of the notice that
    /// says a script did not load.
    fn restart(&mut self) {
        if let Some(s) = self.built.session_mut() {
            s.restart();
        }
        self.reload_script();
    }

    fn step(&mut self, dt: Duration, events: &[Event], stats: FrameStats) -> bool {
        self.since_watch += dt;
        if self.since_watch >= WATCH_EVERY {
            self.since_watch = Duration::ZERO;
            self.reload_script();
        }
        self.built.session_mut().is_some_and(|s| {
            s.set_stats(stats);
            s.step(dt, events).quit
        })
    }

    /// The session keeps its running game; only what it draws with changes.
    fn relight(&mut self, lit: Built) {
        self.gpu.level_changed();
        match self.built.session_mut() {
            Some(session) => session.set_renderer(lit.into_renderer()),
            None => self.built = lit,
        }
    }

    fn apply_voxels(&mut self, change: VoxelChange) {
        self.gpu.level_changed();
        self.built.renderer_mut().apply_voxels(change);
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        if req.play {
            gpu_view::set_inactive();
            return self.built.session_mut()?.render(req.w, req.h);
        }
        // The editor shows the level, not what the last run did to it.
        if let Some(session) = self.built.session_mut() {
            session.restore_voxels();
        }
        if self.fb.as_ref().map(|f| (f.width(), f.height())) != Some((req.w, req.h)) {
            self.fb = Some(Framebuffer::new(req.w, req.h)?);
        }
        let fb = self.fb.as_mut()?;
        let renderer = self.built.renderer();
        let frame = FrameInput {
            view: &req.view,
            quality: Quality::High,
            actors: renderer.actors(),
            effects: Effects::default(),
            wants_depth: !req.highlights.is_empty(),
        };
        if !self.gpu.draw(renderer.assets(), fb, &frame) {
            renderer.render(fb, &req.view, Quality::High, renderer.actors(), None)?;
        }
        if !req.highlights.is_empty()
            && let Some(cam) = req
                .view
                .camera(renderer.config().fov_y, 0.1, 400.0, req.w, req.h)
        {
            highlight::draw(fb, &cam, &req.highlights);
        }
        if let Some(banner) = req.banner {
            draw_text(fb, 6, 6, banner, 0xffff_c040, 1);
        }
        self.fb.as_ref()
    }
}
