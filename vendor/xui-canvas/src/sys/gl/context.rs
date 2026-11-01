//! A `glutin` OpenGL context and window surface for a `winit` window, plus the
//! `glow` loader over its entry points.
//!
//! Creation picks a double-buffered RGBA config with depth and stencil, creates
//! a core-profile 3.3 context on the window's raw handles and makes it current,
//! then resolves `glow`'s functions through the display. Any failure is a
//! [`String`] the caller turns into the widget's permanent software fallback.

use std::ffi::CString;
use std::num::NonZeroU32;
use std::ptr;

use glow::HasContext;
use glutin::config::{ConfigTemplateBuilder, GlConfig};
use glutin::context::{
    ContextApi, ContextAttributesBuilder, GlProfile, NotCurrentGlContext, PossiblyCurrentContext,
    Version,
};
use glutin::display::{Display, DisplayApiPreference, GlDisplay};
use glutin::prelude::*;
use glutin::surface::{Surface, SurfaceAttributesBuilder, SwapInterval, WindowSurface};
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use xui_core::color::Color;

/// A current OpenGL context and the window surface it draws to.
pub(crate) struct Context {
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    glow: glow::Context,
}

impl Context {
    /// Creates a context on `window`'s client area of `width` x `height`
    /// physical pixels.
    pub(crate) fn new(window: &Window, width: u32, height: u32) -> Result<Context, String> {
        let raw_window = window
            .window_handle()
            .map_err(|error| format!("no window handle: {error}"))?
            .as_raw();
        let raw_display = window
            .display_handle()
            .map_err(|error| format!("no display handle: {error}"))?
            .as_raw();

        // SAFETY: the window keeps its native handles alive for the call, and
        // the preference matches the compiled glutin backends for this target.
        let display = unsafe { Display::new(raw_display, preference(raw_window)) }
            .map_err(|error| format!("no OpenGL display: {error}"))?;

        let template = ConfigTemplateBuilder::new()
            .with_alpha_size(8)
            .with_depth_size(24)
            .with_stencil_size(8)
            .build();
        // SAFETY: `display` is live; the template carries no dangling native
        // window.
        let config = unsafe { display.find_configs(template) }
            .map_err(|error| format!("no OpenGL config: {error}"))?
            .reduce(|a, b| {
                if b.num_samples() > a.num_samples() {
                    b
                } else {
                    a
                }
            })
            .ok_or_else(|| "the driver offers no matching OpenGL config".to_string())?;

        let (Some(width), Some(height)) = (
            NonZeroU32::new(width.max(1)),
            NonZeroU32::new(height.max(1)),
        ) else {
            return Err("zero-sized window".to_string());
        };
        let surface_attributes =
            SurfaceAttributesBuilder::<WindowSurface>::new().build(raw_window, width, height);
        // SAFETY: `config` came from `display`, `surface_attributes` names the
        // live window and a non-zero size.
        let surface = unsafe { display.create_window_surface(&config, &surface_attributes) }
            .map_err(|error| format!("no OpenGL window surface: {error}"))?;

        let context_attributes = ContextAttributesBuilder::new()
            .with_profile(GlProfile::Core)
            .with_context_api(ContextApi::OpenGl(Some(Version::new(3, 3))))
            .build(Some(raw_window));
        // SAFETY: `config` belongs to `display`, and the attributes request a
        // core profile the config was selected for.
        let not_current = unsafe { display.create_context(&config, &context_attributes) }
            .map_err(|error| format!("no OpenGL context: {error}"))?;
        let context = not_current
            .make_current(&surface)
            .map_err(|error| format!("cannot make the OpenGL context current: {error}"))?;

        // SAFETY: a context is current, as `glow` requires to load entry points;
        // the closure only resolves names through the same live display.
        let glow = unsafe {
            glow::Context::from_loader_function(|name| {
                let Ok(name) = CString::new(name) else {
                    return ptr::null();
                };
                display.get_proc_address(name.as_c_str())
            })
        };

        // Best effort: a driver without swap control ignores it.
        let _ = surface.set_swap_interval(&context, SwapInterval::Wait(NonZeroU32::MIN));

        Ok(Context {
            context,
            surface,
            glow,
        })
    }

    /// The `glow` wrapper over this context's entry points.
    pub(crate) fn glow(&self) -> &glow::Context {
        &self.glow
    }

    /// Makes this context current on the calling thread.
    pub(crate) fn make_current(&self) {
        let _ = self.context.make_current(&self.surface);
    }

    /// Resizes the window surface to `width` x `height` physical pixels.
    pub(crate) fn resize(&self, width: u32, height: u32) {
        if let (Some(width), Some(height)) = (
            NonZeroU32::new(width.max(1)),
            NonZeroU32::new(height.max(1)),
        ) {
            self.surface.resize(&self.context, width, height);
        }
    }

    /// Sets the viewport to the whole framebuffer.
    pub(crate) fn set_viewport(&self, width: i32, height: i32) {
        // SAFETY: a context is current and the viewport extents are
        // non-negative.
        unsafe { self.glow.viewport(0, 0, width.max(0), height.max(0)) };
    }

    /// Clears the colour and depth buffers to `background`.
    pub(crate) fn clear_to(&self, background: Color) {
        // SAFETY: a context is current; the constants are valid clear bits.
        unsafe {
            self.glow.clear_color(
                f32::from(background.r) / 255.0,
                f32::from(background.g) / 255.0,
                f32::from(background.b) / 255.0,
                1.0,
            );
            self.glow
                .clear(glow::COLOR_BUFFER_BIT | glow::DEPTH_BUFFER_BIT);
        }
    }

    /// Presents the back buffer.
    pub(crate) fn swap(&self) -> Result<(), String> {
        self.surface
            .swap_buffers(&self.context)
            .map_err(|error| format!("cannot present the OpenGL frame: {error}"))
    }
}

impl xui_gpu::GlContext for Context {
    fn make_current(&self) {
        Context::make_current(self);
    }

    fn glow(&self) -> &glow::Context {
        Context::glow(self)
    }

    fn set_viewport(&self, width: i32, height: i32) {
        Context::set_viewport(self, width, height);
    }

    fn clear_to(&self, background: Color) {
        Context::clear_to(self, background);
    }

    fn present(&self) -> Result<(), String> {
        self.swap()
    }

    fn prepare_frame(&self, width: u32, height: u32) -> Option<u32> {
        self.resize(width, height);
        None
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        // Release the thread's current binding before the surface and context
        // are destroyed, so it never points at freed memory.
        let _ = self.context.make_not_current_in_place();
    }
}

/// The display backend to try for this target.
#[cfg(target_os = "windows")]
fn preference(raw_window: RawWindowHandle) -> DisplayApiPreference {
    DisplayApiPreference::WglThenEgl(Some(raw_window))
}

/// The display backend to try for this target.
#[cfg(target_os = "macos")]
fn preference(_raw_window: RawWindowHandle) -> DisplayApiPreference {
    DisplayApiPreference::Cgl
}

/// The display backend to try for this target.
#[cfg(all(unix, not(target_os = "macos")))]
fn preference(_raw_window: RawWindowHandle) -> DisplayApiPreference {
    DisplayApiPreference::EglThenGlx(Box::new(winit::platform::x11::register_xlib_error_hook))
}
