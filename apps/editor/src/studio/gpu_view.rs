//! The 3D pane on the GPU. It draws on the render thread and reads back into the
//! same framebuffer the software path fills, so the mailbox, the pane and the UI
//! thread do not change (`docs/hardware-renderer.md`). Whatever it cannot do, the
//! software renderer does instead, and the log says why.
//!
//! Off until `set_wanted` turns it on, as `main` does: tests read exact pixels,
//! and only the software path gives the same ones on every machine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

use mscent_gpu::{Finish, Gpu, GpuRenderer};
use mscent_render::camera::MAX_SIDE;
use mscent_render::{FrameInput, Framebuffer, LevelAssets, Quality};

static WANTED: AtomicBool = AtomicBool::new(false);
/// Whether the last frame came from the GPU, which sets how large the pane draws.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static DEVICE: Devices = Devices::new();
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

/// Until a frame says otherwise: on entering Play, which software draws where
/// the GPU cannot. Once, not at every frame: the pane's frame size follows this
/// flag, and a size that flips remakes the GPU's targets each time.
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

/// A device for the life of the editor, as every edit brings a new scene; or
/// why there is none. Opened when first asked for, and a lost one opened again
/// once: a GPU that keeps going is left to software.
pub(super) struct Devices {
    slot: Mutex<Option<Result<Gpu, String>>>,
    reopened: AtomicBool,
}

impl Devices {
    pub(super) const fn new() -> Self {
        Self {
            slot: Mutex::new(None),
            reopened: AtomicBool::new(false),
        }
    }

    /// The device, opened with `open` if it has to be.
    pub(super) fn get(&self, open: impl FnOnce() -> Result<Gpu, String>) -> Result<Gpu, String> {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let current = slot.as_ref().and_then(|opened| opened.as_ref().ok());
        if let Some(why) = current.and_then(Gpu::lost) {
            let gone = format!("the GPU device was lost: {why}");
            let again = !self.reopened.swap(true, Ordering::Relaxed);
            *slot = (!again).then_some(Err(gone));
        }
        slot.get_or_insert_with(open).clone()
    }

    /// Whether the device has gone and can still be opened again.
    pub(super) fn can_reopen(&self) -> bool {
        let slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let current = slot.as_ref().and_then(|opened| opened.as_ref().ok());
        current.is_some_and(|gpu| gpu.lost().is_some()) && !self.reopened.load(Ordering::Relaxed)
    }
}

fn device() -> Result<Gpu, String> {
    DEVICE.get(|| Gpu::new().map_err(|e| e.to_string()))
}

/// The internal render size for a pane of `pane` pixels: the pane divided by
/// the smallest whole factor that brings the width to `max_width` or less, and
/// both sides to the most a framebuffer may have, which a pane wider than two
/// monitors would pass.
pub fn frame_size(pane: (u32, u32), max_width: u32) -> (u32, u32) {
    let (w, h) = (pane.0.max(1), pane.1.max(1));
    let width_factor = w.div_ceil(max_width.clamp(1, MAX_SIDE));
    let factor = width_factor.max(h.div_ceil(MAX_SIDE)).max(1);
    ((w / factor).max(16), (h / factor).max(16))
}

/// One scene's level on the GPU, made when first drawn.
#[derive(Default)]
pub struct GpuView {
    level: Option<GpuRenderer>,
    /// Why the GPU failed on this scene, which software draws from here on.
    failed: Option<String>,
    /// Draws whatever `set_wanted` says, for a test among parallel ones.
    #[cfg(test)]
    pub forced: bool,
    /// The GPU-only looks a test sets, where it holds the picture to software's.
    #[cfg(test)]
    pub effects: Option<mscent_gpu::GpuEffects>,
    /// A device of the test's own, which it may lose without taking the
    /// editor's from the tests beside it.
    #[cfg(test)]
    pub own_device: Option<Gpu>,
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

    /// Why the GPU gave this scene up, if it has.
    #[cfg(test)]
    pub fn failure(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    fn give_up(&mut self, why: &str) {
        self.level = None;
        self.failed = Some(why.to_string());
        self.say(format!("3D view: software renderer, because {why}"));
    }

    /// The test's own device, or the editor's.
    fn device(&self) -> Result<Gpu, String> {
        #[cfg(test)]
        if let Some(gpu) = &self.own_device {
            return Ok(gpu.clone());
        }
        device()
    }

    /// For a frame the GPU failed: the level is made again on a new device
    /// the next frame if the device was lost for the first time, and otherwise
    /// left to software.
    fn failed_frame(&mut self, why: &str) {
        #[cfg(test)]
        let reopen = self.own_device.is_none() && DEVICE.can_reopen();
        #[cfg(not(test))]
        let reopen = DEVICE.can_reopen();
        if reopen {
            self.level = None;
            self.say(format!("3D view: {why}; opening the GPU again"));
        } else {
            self.give_up(why);
        }
    }

    /// The level on the GPU with the finish `quality` gets there, made if it has
    /// to be. `None` leaves it to software.
    fn level(&mut self, assets: &LevelAssets, quality: Quality) -> Option<&mut GpuRenderer> {
        let gpu = match self.device() {
            Ok(gpu) => gpu,
            Err(why) => {
                self.give_up(&why);
                return None;
            }
        };
        let finish = Finish::of(quality, &gpu);
        if self.level.as_ref().map(GpuRenderer::finish) != Some(finish) {
            match GpuRenderer::with_finish(gpu, assets, finish) {
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
        if !self.is_wanted() || self.failed.is_some() {
            return false;
        }
        #[cfg(test)]
        let effects = self.effects;
        let Some(level) = self.level(assets, frame.quality) else {
            return false;
        };
        #[cfg(test)]
        level.set_effects(effects);
        let drawn = level.draw(assets, fb, frame).is_some();
        match level.take_error() {
            Some(why) => self.failed_frame(&why),
            None if !drawn => self.failed_frame("the GPU drew no frame"),
            None => {
                if let Ok(gpu) = self.device() {
                    self.say(format!("3D view: {}", gpu.adapter()));
                }
                return true;
            }
        }
        false
    }
}
