//! The real [`Scene`]: a built level, drawn as the editor's fly camera view
//! or as the running game.

use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::{Framebuffer, Quality, draw_text};

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
        if req.stale {
            draw_text(fb, 6, 6, "OUT OF DATE: F7", 0xffff_c040, 1);
        }
        self.fb.as_ref()
    }
}
