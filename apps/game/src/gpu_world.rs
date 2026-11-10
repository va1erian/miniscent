//! The game's world on the GPU. Given a window, the GPU shows each frame in it
//! with the 2D layer and the white flash over it, reading nothing back (the
//! direct route). Without one, it draws into the frame software would fill and
//! reads it back, so what goes over it draws as before and the particle budget
//! reads its depth next frame. Whatever the GPU cannot do, software does
//! instead, and stderr says why once.

#[cfg(feature = "desktop")]
use std::sync::Arc;

use mscent_gpu::{Finish, Gpu, GpuRenderer};
#[cfg(feature = "desktop")]
use mscent_gpu::{Overlay, Presenter};
#[cfg(feature = "desktop")]
use mscent_platform::{GpuWindow, scale::fit_rect};
use mscent_render::{FrameInput, Framebuffer, LevelAssets};

#[cfg(feature = "desktop")]
use crate::app::Shown;
use crate::app::World;

/// The GPU, opened on the first frame, and the session's level on it.
pub struct GpuWorld {
    gpu: Option<Gpu>,
    level: Option<GpuRenderer>,
    /// Why the GPU gave up, after which software draws.
    failed: Option<String>,
    /// `--renderer hardware`: a failure stops the run instead.
    strict: bool,
    /// The window it shows frames in, and its surface there once made.
    #[cfg(feature = "desktop")]
    window: Option<Arc<dyn GpuWindow>>,
    #[cfg(feature = "desktop")]
    presenter: Option<Presenter>,
}

impl GpuWorld {
    pub fn new(strict: bool) -> Self {
        Self {
            gpu: None,
            level: None,
            failed: None,
            strict,
            #[cfg(feature = "desktop")]
            window: None,
            #[cfg(feature = "desktop")]
            presenter: None,
        }
    }

    /// Showing its frames in `window` itself.
    #[cfg(feature = "desktop")]
    pub fn in_window(self, window: Arc<dyn GpuWindow>) -> Self {
        Self {
            window: Some(window),
            ..self
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

    /// Draws `frame` and shows it in the window with `over` over it.
    #[cfg(feature = "desktop")]
    fn try_show(
        &mut self,
        assets: &LevelAssets,
        frame: &FrameInput,
        over: &Shown,
    ) -> Result<(), String> {
        self.level(assets, frame)?;
        let gpu = self.gpu.as_ref().ok_or("no GPU")?;
        if self.presenter.is_none() {
            let window = self.window.clone().ok_or("no window")?;
            self.presenter = Some(Presenter::new(gpu, window).map_err(|e| e.to_string())?);
        }
        let (Some(level), Some(presenter)) = (self.level.as_mut(), self.presenter.as_mut()) else {
            return Err("no level".into());
        };
        let size = over.world;
        let overlay = Overlay {
            layer: over.layer.pixels(),
            layer_size: (over.layer.width(), over.layer.height()),
            white: over.white,
            window: over.window,
            rect: fit_rect(size.0, size.1, over.window.0, over.window.1),
        };
        let drawn = level.draw_and_present(assets, size, frame, (presenter, &overlay));
        if let Some(why) = level.take_error() {
            return Err(why);
        }
        drawn
            .map(|_| ())
            .ok_or_else(|| "the GPU drew no frame".into())
    }

    /// Why the GPU was given up, if it was.
    pub fn failure(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// Gives up the GPU for `why`, after which software draws.
    fn fail(&mut self, why: String) {
        if !self.strict {
            eprintln!("renderer: software, because {why}");
        }
        self.level = None;
        // The window's GPU surface goes, so that the platform's can take it.
        #[cfg(feature = "desktop")]
        {
            self.presenter = None;
        }
        self.failed = Some(why);
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
        self.fail(why);
        false
    }

    #[cfg(feature = "desktop")]
    fn shows(&self) -> bool {
        self.window.is_some() && self.failed.is_none()
    }

    #[cfg(feature = "desktop")]
    fn show(&mut self, assets: &LevelAssets, frame: &FrameInput, over: &Shown) -> bool {
        let Err(why) = self.try_show(assets, frame, over) else {
            return true;
        };
        self.fail(why);
        false
    }

    #[cfg(feature = "desktop")]
    fn waits_for_display(&self) -> bool {
        self.presenter
            .as_ref()
            .is_some_and(Presenter::waits_for_display)
    }

    fn stop(&self) -> Option<String> {
        self.failed.clone().filter(|_| self.strict)
    }
}
