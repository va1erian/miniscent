//! The game's world drawn on the GPU and read back into the frame software
//! would fill, so the HUD, menus, feedback and the white flash draw on it as
//! before, and the particle budget reads its depth next frame. Whatever the GPU
//! cannot do, software does instead, and stderr says why once.

use mscent_gpu::{Finish, Gpu, GpuRenderer};
use mscent_render::{FrameInput, Framebuffer, LevelAssets};

use crate::app::World;

/// The GPU, opened on the first frame, and the session's level on it.
pub struct GpuWorld {
    gpu: Option<Gpu>,
    level: Option<GpuRenderer>,
    /// Why the GPU gave up, after which software draws.
    failed: Option<String>,
    /// `--renderer hardware`: a failure stops the run instead.
    strict: bool,
}

impl GpuWorld {
    pub fn new(strict: bool) -> Self {
        Self {
            gpu: None,
            level: None,
            failed: None,
            strict,
        }
    }

    /// On a GPU already open, which a test can then take away.
    pub fn on(gpu: Gpu, strict: bool) -> Self {
        Self {
            gpu: Some(gpu),
            ..Self::new(strict)
        }
    }

    /// The level on the GPU with the finish `frame` gets there, made if it has
    /// to be: on the first frame, and when the quality changes the finish.
    fn level(
        &mut self,
        assets: &LevelAssets,
        frame: &FrameInput,
    ) -> Result<&mut GpuRenderer, String> {
        let gpu = match &mut self.gpu {
            Some(gpu) => gpu,
            slot => slot.insert(Gpu::new().map_err(|e| e.to_string())?),
        };
        let finish = Finish::of(frame.quality, gpu);
        if self.level.as_ref().map(GpuRenderer::finish) != Some(finish) {
            let level = GpuRenderer::with_finish(gpu.clone(), assets, finish);
            self.level = Some(level.map_err(|e| e.to_string())?);
            eprintln!("renderer: GPU, {} ({finish:?})", gpu.adapter());
        }
        self.level.as_mut().ok_or_else(|| "no level".into())
    }

    fn try_draw(
        &mut self,
        assets: &LevelAssets,
        fb: &mut Framebuffer,
        frame: &FrameInput,
    ) -> Result<(), String> {
        let level = self.level(assets, frame)?;
        let drawn = level.draw(assets, fb, frame);
        if let Some(why) = level.take_error() {
            return Err(why);
        }
        drawn
            .map(|_| ())
            .ok_or_else(|| "the GPU drew no frame".into())
    }
}

impl World for GpuWorld {
    fn draw(&mut self, assets: &LevelAssets, fb: &mut Framebuffer, frame: &FrameInput) -> bool {
        if self.failed.is_some() {
            return false;
        }
        let Err(why) = self.try_draw(assets, fb, frame) else {
            return true;
        };
        if !self.strict {
            eprintln!("renderer: software, because {why}");
        }
        self.level = None;
        self.failed = Some(why);
        false
    }

    fn stop(&self) -> Option<String> {
        self.failed.clone().filter(|_| self.strict)
    }
}
