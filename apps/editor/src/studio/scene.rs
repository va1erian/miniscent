//! The real [`Scene`]: a built level, drawn as the editor's fly camera view
//! or as the running game.

use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Framebuffer, Quality, draw_text};

use super::highlight;
use super::worker::{Request, Scene};
use crate::build::Built;

pub struct LevelScene {
    built: Built,
    fb: Option<Framebuffer>,
}

impl LevelScene {
    pub fn new(built: Built) -> LevelScene {
        LevelScene { built, fb: None }
    }

    /// The running game, for tests that look inside.
    #[cfg(test)]
    pub fn session(&mut self) -> Option<&mut miniscent_game::GameSession> {
        self.built.session_mut()
    }
}

impl Scene for LevelScene {
    fn restart(&mut self) {
        if let Some(s) = self.built.session_mut() {
            s.restart();
        }
    }

    fn step(&mut self, dt: Duration, events: &[Event], stats: FrameStats) -> bool {
        self.built.session_mut().is_some_and(|s| {
            s.set_stats(stats);
            s.step(dt, events).quit
        })
    }

    /// The session keeps its running game; only what it draws with changes.
    fn relight(&mut self, lit: Built) {
        match self.built.session_mut() {
            Some(session) => session.set_renderer(lit.into_renderer()),
            None => self.built = lit,
        }
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        if req.play {
            return self.built.session_mut()?.render(req.w, req.h);
        }
        if self.fb.as_ref().map(|f| (f.width(), f.height())) != Some((req.w, req.h)) {
            self.fb = Some(Framebuffer::new(req.w, req.h)?);
        }
        let fb = self.fb.as_mut()?;
        let renderer = self.built.renderer();
        renderer.render(fb, &req.view, Quality::High, renderer.actors(), None)?;
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
