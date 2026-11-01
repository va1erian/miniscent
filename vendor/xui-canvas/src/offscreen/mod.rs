#![forbid(unsafe_code)]

//! An offscreen [`xui_core::backend::Backend`] that composites the portable
//! widgets into a software [`Surface`] instead of a window.
//!
//! It runs on every platform with no UI toolkit, so the same widget code that
//! the Win32 backend hosts can be rendered and snapshot-tested here (including
//! on headless CI runners). The windowing shell builds on the same surface.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use xui_core::backend::{Event, PlatformSpec, Result as BackendResult, WidgetId, WindowId};
use xui_core::router::WidgetHost;
use xui_core::{Image, Theme};

use crate::Surface;
use crate::text_layout::CosmicShaper;

mod backend;
#[cfg(test)]
mod capture_tests;
mod geometry;
mod gl_fallback;
mod input;
mod present;
#[cfg(test)]
mod tests;

use gl_fallback::GlContent;
pub use present::PresentedFrame;

use crate::scene::{Node, Scene};

/// The default dots-per-inch a surface is rendered at.
const DEFAULT_DPI: u32 = 96;

struct OffscreenWindow {
    /// The window's persistent frame; `None` only while a frame is being
    /// painted (see [`OffscreenBackend::present`]).
    surface: Option<Surface>,
    sink: Option<Rc<dyn WidgetHost>>,
    theme: Theme,
    dpi: u32,
    width: i32,
    height: i32,
    /// Window- and node-level GL content, painted through its software fallback
    /// (the offscreen backend never has a GL context). A no-op when the
    /// `winit-backend` feature is off.
    gl: GlContent,
    /// The node the pointer is over, so a move to another tells it it left.
    hover: Option<WidgetId>,
    /// The last icon the app set with `Backend::set_window_icon`.
    icon: Option<Image>,
}

/// A backend that renders into a software surface.
///
/// It reports no reduced-motion preference by default, whatever the machine's
/// setting, so a headless render is the same everywhere. A test that exercises
/// the reduced-motion path sets it with
/// [`OffscreenBackend::set_prefers_reduced_motion`].
pub struct OffscreenBackend {
    windows: RefCell<HashMap<u64, OffscreenWindow>>,
    nodes: RefCell<Scene>,
    next_window: std::cell::Cell<u64>,
    next_widget: std::cell::Cell<u64>,
    /// The node the pointer is captured by, if any.
    captured: RefCell<Option<WidgetId>>,
    /// The node with keyboard focus, which receives injected key and character
    /// events as it would from a real backend.
    focused: RefCell<Option<WidgetId>>,
    /// The windows currently being composited, so a painter that captures its
    /// own window (re-entering `render`) is refused instead of recursing.
    rendering: RefCell<HashSet<u64>>,
    text: OnceCell<CosmicShaper>,
    /// The dots-per-inch `Backend::open_window` renders new windows at.
    dpi: u32,
    /// Work to do when the loop "runs": a scripted session for a headless
    /// render, taken and executed once by `Backend::run`.
    run_hook: RefCell<Option<Box<dyn FnOnce()>>>,
    /// What `Backend::prefers_reduced_motion` answers; `false` unless a test
    /// scripts it with [`OffscreenBackend::set_prefers_reduced_motion`].
    prefers_reduced_motion: std::cell::Cell<bool>,
}

impl Default for OffscreenBackend {
    fn default() -> OffscreenBackend {
        OffscreenBackend::new()
    }
}

impl OffscreenBackend {
    /// A backend with no windows.
    pub fn new() -> OffscreenBackend {
        OffscreenBackend {
            windows: RefCell::new(HashMap::new()),
            nodes: RefCell::new(Scene::new()),
            next_window: std::cell::Cell::new(1),
            next_widget: std::cell::Cell::new(1),
            captured: RefCell::new(None),
            focused: RefCell::new(None),
            rendering: RefCell::new(HashSet::new()),
            text: OnceCell::new(),
            dpi: DEFAULT_DPI,
            run_hook: RefCell::new(None),
            prefers_reduced_motion: std::cell::Cell::new(false),
        }
    }

    /// A backend whose windows are rendered at `dpi` (the layout is in design
    /// units, so a higher value gives a proportionally larger image).
    pub fn with_dpi(dpi: u32) -> OffscreenBackend {
        OffscreenBackend {
            dpi,
            ..OffscreenBackend::new()
        }
    }

    /// Sets the work `Backend::run` performs. The offscreen loop has nothing
    /// to wait for, so `run` executes this once, after the app is built and
    /// while its window is still open, then returns.
    pub fn set_run_hook(&self, hook: impl FnOnce() + 'static) {
        *self.run_hook.borrow_mut() = Some(Box::new(hook));
    }

    /// Scripts the answer to
    /// [`Backend::prefers_reduced_motion`](xui_core::backend::Backend::prefers_reduced_motion),
    /// so a headless test or render can exercise an app's reduced-motion path.
    /// It is `false` until set: the offscreen backend never reads the
    /// machine's setting, which keeps a headless render deterministic.
    pub fn set_prefers_reduced_motion(&self, reduced: bool) {
        self.prefers_reduced_motion.set(reduced);
    }

    /// The last icon set on `window` with
    /// [`Backend::set_window_icon`](xui_core::backend::Backend::set_window_icon),
    /// so an app's tests can check that it set one.
    pub fn window_icon(&self, window: WindowId) -> Option<Image> {
        self.windows
            .borrow()
            .get(&window.raw())
            .and_then(|entry| entry.icon.clone())
    }

    /// How many windows are open.
    pub fn window_count(&self) -> usize {
        self.windows.borrow().len()
    }

    /// Delivers a wake to `window`'s sink, so queued messages reach the app's
    /// `update` as they would when a window system wakes the loop.
    pub fn pump(&self, window: WindowId) {
        let sink = self
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|entry| entry.sink.clone());
        if let Some(sink) = sink {
            sink.deliver(WidgetId::NONE, &Event::Wake);
        }
    }

    fn allocate(cell: &std::cell::Cell<u64>) -> u64 {
        let id = cell.get();
        cell.set(id + 1);
        id
    }

    /// Opens a window rendered at `dpi`, so a test can exercise high-DPI
    /// layouts. [`xui_core::backend::Backend::open_window`] uses
    /// the backend's own DPI.
    pub fn open_window_at(&self, spec: &PlatformSpec, dpi: u32) -> BackendResult<WindowId> {
        let id = WindowId::from_raw(Self::allocate(&self.next_window));
        let width = spec.width.to_px(dpi).value().max(1) as u32;
        let height = spec.height.to_px(dpi).value().max(1) as u32;
        self.windows.borrow_mut().insert(
            id.raw(),
            OffscreenWindow {
                surface: Some(Surface::new(width, height)),
                sink: None,
                theme: Theme::light(),
                dpi,
                width: width as i32,
                height: height as i32,
                gl: GlContent::new(),
                hover: None,
                icon: None,
            },
        );
        Ok(id)
    }

    /// Installs `widget` as `window`'s GL content. The offscreen backend has no
    /// GPU, so [`OffscreenBackend::render`] paints the widget's software
    /// fallback; this mirrors the windowed backend's seam and lets a headless
    /// test exercise the fallback.
    ///
    /// The `winit-backend` feature (with it the [`GlWidget`] seam) is required.
    #[cfg(feature = "winit-backend")]
    pub fn set_gl_content<W: crate::gl::GlWidget + 'static>(&self, window: WindowId, widget: W) {
        if let Some(entry) = self.windows.borrow_mut().get_mut(&window.raw()) {
            entry.gl.set_window(widget);
        }
        self.invalidate_window(window);
    }

    /// Removes `window`'s GL content.
    #[cfg(feature = "winit-backend")]
    pub fn clear_gl_content(&self, window: WindowId) {
        if let Some(entry) = self.windows.borrow_mut().get_mut(&window.raw()) {
            entry.gl.clear_window();
        }
        self.invalidate_window(window);
    }

    /// Installs `widget` as the GL content of the node `id`, painted through its
    /// software fallback at the node's bounds. Mirrors the windowed backend's
    /// per-node seam and lets a headless test exercise the fallback in a pane.
    ///
    /// The `winit-backend` feature (with it the [`GlWidget`] seam) is required.
    #[cfg(feature = "winit-backend")]
    pub fn set_gl_content_on<W: crate::gl::GlWidget + 'static>(&self, id: WidgetId, widget: W) {
        let window = self.nodes.borrow().window_of(id);
        if let Some(window) = window
            && let Some(entry) = self.windows.borrow_mut().get_mut(&window.raw())
        {
            entry.gl.set_node(id, widget);
            self.invalidate_window(window);
        }
    }

    /// Removes the GL content of the node `id`.
    #[cfg(feature = "winit-backend")]
    pub fn clear_gl_content_on(&self, id: WidgetId) {
        let window = self.nodes.borrow().window_of(id);
        if let Some(window) = window
            && let Some(entry) = self.windows.borrow_mut().get_mut(&window.raw())
        {
            entry.gl.clear_node(id);
            self.invalidate_window(window);
        }
    }

    /// Forgets the focus when its node is gone.
    pub(super) fn forget_focus_if_gone(&self) {
        let mut focused = self.focused.borrow_mut();
        if let Some(id) = *focused
            && !self.nodes.borrow().contains(id)
        {
            *focused = None;
        }
    }

    /// The dots-per-inch `window` is rendered at.
    pub fn dpi_of(&self, window: WindowId) -> u32 {
        self.windows
            .borrow()
            .get(&window.raw())
            .map_or(DEFAULT_DPI, |entry| entry.dpi)
    }

    fn with_node<R>(&self, id: WidgetId, f: impl FnOnce(&mut Node) -> R) -> Option<R> {
        self.nodes.borrow_mut().get_mut(id).map(f)
    }
}
