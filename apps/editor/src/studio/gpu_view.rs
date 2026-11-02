//! The 3D pane on the GPU. It draws on the render thread and reads back into the
//! same framebuffer the software path fills, so the mailbox, the pane and the UI
//! thread do not change (`docs/hardware-renderer.md`). Whatever it cannot do, the
//! software renderer does instead, and the log says why.
//!
//! Off until `set_wanted` turns it on, as `main` does: tests read exact pixels,
//! and only the software path gives the same ones on every machine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};

use mscent_gpu::{Gpu, GpuRenderer};
use mscent_render::{FrameInput, Framebuffer, LevelAssets};

static WANTED: AtomicBool = AtomicBool::new(false);
/// Whether the last frame came from the GPU, which sets how large the pane draws.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// One device for the life of the editor: every edit brings a new scene.
static DEVICE: OnceLock<Result<Gpu, String>> = OnceLock::new();
/// What the view last said, and whether the log has seen it.
static REPORT: Mutex<(String, bool)> = Mutex::new((String::new(), true));

/// Asks for the GPU from the next frame on, or for software.
pub fn set_wanted(on: bool) {
    WANTED.store(on, Ordering::Relaxed);
    if !on {
        report("3D view: software renderer".to_string());
    }
}

pub fn wanted() -> bool {
    WANTED.load(Ordering::Relaxed)
}

/// Whether the GPU drew the last frame.
pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// For a frame drawn some other way, as Play's is.
pub fn set_inactive() {
    ACTIVE.store(false, Ordering::Relaxed);
}

/// A line for the log, once, when the view's account of itself has changed.
pub fn take_report() -> Option<String> {
    let mut slot = REPORT.lock().unwrap_or_else(PoisonError::into_inner);
    (!std::mem::replace(&mut slot.1, true)).then(|| slot.0.clone())
}

fn report(line: String) {
    let mut slot = REPORT.lock().unwrap_or_else(PoisonError::into_inner);
    if slot.0 != line {
        *slot = (line, false);
    }
}

fn device() -> Result<&'static Gpu, &'static str> {
    let opened = DEVICE.get_or_init(|| Gpu::new().map_err(|e| e.to_string()));
    opened.as_ref().map_err(String::as_str)
}

/// The internal render size for a pane of `pane` pixels: the pane divided by
/// the smallest whole factor that brings the width to `max_width` or less.
pub fn frame_size(pane: (u32, u32), max_width: u32) -> (u32, u32) {
    let (w, h) = (pane.0.max(1), pane.1.max(1));
    let factor = w.div_ceil(max_width.max(1)).max(1);
    ((w / factor).max(16), (h / factor).max(16))
}

/// One scene's level on the GPU, made when first drawn.
#[derive(Default)]
pub struct GpuView {
    level: Option<GpuRenderer>,
    /// The GPU failed on this scene, so software draws it from here on.
    failed: bool,
    /// Draws whatever `set_wanted` says, for a test among parallel ones.
    #[cfg(test)]
    pub forced: bool,
}

impl GpuView {
    fn is_wanted(&self) -> bool {
        self.is_forced() || wanted()
    }

    /// A forced view says nothing and sets nothing: what the view says and how
    /// large the pane draws are the whole editor's, and it runs beside other tests.
    fn is_forced(&self) -> bool {
        #[cfg(test)]
        return self.forced;
        #[cfg(not(test))]
        false
    }

    fn say(&self, line: String) {
        if !self.is_forced() {
            report(line);
        }
    }

    /// Forgets what is on the GPU, for a level that has changed.
    pub fn level_changed(&mut self) {
        self.level = None;
    }

    fn give_up(&mut self, why: &str) {
        self.level = None;
        self.failed = true;
        self.say(format!("3D view: software renderer, because {why}"));
    }

    /// The level on the GPU, made if it has to be. `None` leaves it to software.
    fn level(&mut self, assets: &LevelAssets) -> Option<&mut GpuRenderer> {
        if self.level.is_none() {
            let gpu = match device() {
                Ok(gpu) => gpu,
                Err(why) => {
                    self.give_up(why);
                    return None;
                }
            };
            match GpuRenderer::new(gpu.clone(), assets) {
                Ok(level) => self.level = Some(level),
                Err(why) => {
                    self.give_up(&why.to_string());
                    return None;
                }
            }
        }
        self.level.as_mut()
    }

    /// Draws `frame` into `fb`. False leaves it to the software renderer.
    pub fn draw(&mut self, assets: &LevelAssets, fb: &mut Framebuffer, frame: &FrameInput) -> bool {
        let drawn = self.try_draw(assets, fb, frame);
        if !self.is_forced() {
            ACTIVE.store(drawn, Ordering::Relaxed);
        }
        drawn
    }

    fn try_draw(&mut self, assets: &LevelAssets, fb: &mut Framebuffer, frame: &FrameInput) -> bool {
        if !self.is_wanted() || self.failed {
            return false;
        }
        if let Some(why) = GpuRenderer::cannot_draw(assets) {
            self.say(format!(
                "3D view: software renderer for this level, as it has {why}"
            ));
            return false;
        }
        let Some(level) = self.level(assets) else {
            return false;
        };
        let drawn = level.draw(assets, fb, frame).is_some();
        match level.take_error() {
            Some(why) => self.give_up(&why),
            None if !drawn => self.give_up("the GPU drew no frame"),
            None => {
                if let Ok(gpu) = device() {
                    self.say(format!("3D view: {}", gpu.adapter()));
                }
                return true;
            }
        }
        false
    }
}
