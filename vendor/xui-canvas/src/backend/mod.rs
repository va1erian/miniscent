#![forbid(unsafe_code)]

//! The cross-platform window backend: a `winit` window event loop that
//! composites the portable widgets with `tiny-skia` and presents them through
//! `softbuffer`.
//!
//! Nodes are kept in creation order (later created draws on top, and
//! [`Backend::raise`](xui_core::backend::Backend::raise) moves a node to the back
//! of that order). Bounds are parent-relative, as in the Win32 backend; the
//! window composition walks the parent chain to place a node, and input is
//! translated into a node's own client coordinates before it is delivered,
//! matching the contract the portable widgets rely on.

mod app;
mod contract;
mod gl;
mod render;
mod software;
#[cfg(test)]
mod tests;

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use winit::event_loop::{EventLoop, EventLoopProxy};
use winit::raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};
use winit::window::Window;

use xui_core::backend::{Cursor, Decorations, PlatformSpec, WidgetId, WindowId};
use xui_core::router::WidgetHost;
use xui_core::{Dip, Image, Theme};

use crate::geometry;
use crate::gl::{GlWidget, RendererState};
use crate::scene::{Node, Scene};
use crate::text_layout::CosmicShaper;

/// A cloneable window handle for `softbuffer`. `winit::Window` is not `Clone`,
/// so the display and window handles share one `Rc`.
pub(crate) struct SharedWindow(pub(crate) Rc<Window>);

impl HasWindowHandle for SharedWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for SharedWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        self.0.display_handle()
    }
}

/// Converts an RGBA image to a `winit` window icon; `None` if `winit` rejects
/// its dimensions.
pub(crate) fn to_winit_icon(image: &Image) -> Option<winit::window::Icon> {
    winit::window::Icon::from_rgba(image.pixels().to_vec(), image.width(), image.height()).ok()
}

/// The default dots-per-inch a window is rendered at before the platform
/// reports its scale factor.
const DEFAULT_DPI: u32 = 96;

/// A message posted to the event loop from another thread (a
/// [`Waker`](xui_core::backend::Waker)) or the backend itself.
pub(crate) enum UserEvent {
    /// Deliver [`xui_core::backend::Event::Wake`] to a window.
    Wake(WindowId),
}

/// The per-window state.
pub(crate) struct WindowState {
    pub(crate) title: String,
    pub(crate) size: (u32, u32),
    pub(crate) dpi: u32,
    pub(crate) theme: Theme,
    pub(crate) sink: Option<Rc<dyn WidgetHost>>,
    pub(crate) window: Option<Rc<Window>>,
    /// The icon the app asked for. Kept so a window that does not exist yet
    /// gets it when it is created.
    pub(crate) icon: Option<Image>,
    /// Whether the window shows the system title bar.
    pub(crate) decorations: Decorations,
    /// Whether the user may resize the window.
    pub(crate) resizable: bool,
    /// The custom caption band height the app asked for.
    pub(crate) caption_inset: Dip,
    /// The node the pointer is over, so a `MouseLeave` can be sent on exit.
    pub(crate) hover: Option<WidgetId>,
    /// The node with the keyboard focus.
    pub(crate) focused: Option<WidgetId>,
    /// The window-level GPU renderer, when the app installed one. Rendered
    /// offscreen and composited as the base layer of the client area.
    pub(crate) gl: Option<Rc<dyn GlWidget>>,
    /// Per-node GPU renderers, keyed by the node they fill. Each is rendered
    /// offscreen at its node's bounds and composited among the other nodes, so
    /// a GL visualizer can sit in one pane of an ordinary app.
    pub(crate) gl_nodes: HashMap<WidgetId, Rc<dyn GlWidget>>,
    /// The GL surface state shared by the content in `gl` and `gl_nodes`.
    pub(crate) renderer: RendererState,
    /// Whether this is the first window opened on the backend (the app's main
    /// window). An unhandled close on it ends the loop; a secondary window's
    /// close does not.
    pub(crate) primary: bool,
}

impl WindowState {
    /// The `winit` icon for the image the app asked for, if it set one that
    /// `winit` accepts.
    pub(crate) fn winit_icon(&self) -> Option<winit::window::Icon> {
        self.icon.as_ref().and_then(to_winit_icon)
    }

    fn new(spec: &PlatformSpec) -> WindowState {
        WindowState {
            title: spec.title.clone(),
            size: (
                spec.width.to_px(DEFAULT_DPI).value().max(1) as u32,
                spec.height.to_px(DEFAULT_DPI).value().max(1) as u32,
            ),
            dpi: DEFAULT_DPI,
            theme: Theme::light(),
            sink: None,
            window: None,
            icon: None,
            decorations: spec.decorations,
            resizable: spec.resizable,
            caption_inset: spec.caption_inset,
            hover: None,
            focused: None,
            gl: None,
            gl_nodes: HashMap::new(),
            renderer: RendererState::Untried,
            primary: false,
        }
    }
}

/// State every backend method shares with the event-loop handler.
pub(crate) struct Shared {
    pub(crate) windows: RefCell<HashMap<u64, WindowState>>,
    pub(crate) nodes: RefCell<Scene>,
    pub(crate) cursors: RefCell<HashMap<u64, Cursor>>,
    /// The node the pointer is captured by, if any: pointer moves and releases
    /// go to it even outside its bounds.
    pub(crate) captured: RefCell<Option<WidgetId>>,
    /// Repeating timers: window, next due instant and period. A timer fires,
    /// is rescheduled one period on, and lives until killed.
    pub(crate) timers: RefCell<HashMap<usize, (WindowId, Instant, Duration)>>,
    pub(crate) next_window: Cell<u64>,
    pub(crate) next_widget: Cell<u64>,
    pub(crate) next_timer: Cell<usize>,
    pub(crate) proxy: EventLoopProxy<UserEvent>,
    pub(crate) exit_code: Cell<i32>,
    pub(crate) quit: Cell<bool>,
}

impl Shared {
    /// The top-level node under `(x, y)` in window coordinates, topmost first,
    /// with the point translated into that node's own client coordinates.
    pub(crate) fn hit_test(
        &self,
        window: WindowId,
        x: i32,
        y: i32,
    ) -> Option<(WidgetId, i32, i32)> {
        geometry::hit_test(&self.nodes.borrow(), window, x, y)
    }

    /// The point `(x, y)`, in window coordinates, translated into `id`'s own
    /// client coordinates.
    pub(crate) fn local_point(&self, id: WidgetId, x: i32, y: i32) -> Option<(i32, i32)> {
        let nodes = self.nodes.borrow();
        let abs = geometry::absolute_bounds(&nodes, id)?;
        Some((x - abs.left, y - abs.top))
    }

    /// Whether the node `id` is a transient popup surface.
    pub(crate) fn is_popup(&self, id: WidgetId) -> bool {
        self.nodes.borrow().get(id).is_some_and(|node| node.popup)
    }

    /// Whether the node `id` is marked as a window-drag region.
    pub(crate) fn is_drag_region(&self, id: WidgetId) -> bool {
        self.nodes
            .borrow()
            .get(id)
            .is_some_and(|node| node.drag_region)
    }

    /// Sends `event` to the sink of `window`, then marks `target`'s place
    /// stale when the event is one that can change how it looks (an input
    /// event, a focus or capture change, a leave), so a widget that forgets to
    /// invalidate cannot leave old pixels behind, and asks for a frame. The
    /// sink's answer does not matter: it is `true` only when a widget mapped the
    /// event to a message.
    pub(crate) fn deliver(
        &self,
        window: WindowId,
        target: WidgetId,
        event: &xui_core::backend::Event,
    ) -> bool {
        let sink = self
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|state| state.sink.clone());
        let handled = sink.is_some_and(|sink| sink.deliver(target, event));
        if !target.is_none()
            && crate::scene::event_marks_target(event)
            && self.nodes.borrow_mut().invalidate(target)
        {
            self.request_redraw(window);
        }
        handled
    }

    /// Marks the whole of `window` stale and requests a frame of it: a
    /// resize, a DPI or theme change, or GL content that came or went.
    pub(crate) fn invalidate_window(&self, window: WindowId) {
        self.nodes.borrow_mut().invalidate_window(window);
        self.request_redraw(window);
    }

    /// Requests a repaint of `window`.
    pub(crate) fn request_redraw(&self, window: WindowId) {
        // Clone the handle out so no guard is live while `winit` is called.
        let handle = self
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|state| state.window.clone());
        if let Some(handle) = handle {
            handle.request_redraw();
        }
    }
}

/// The `winit` + `tiny-skia` backend.
pub struct WinitBackend {
    shared: Rc<Shared>,
    event_loop: RefCell<Option<EventLoop<UserEvent>>>,
    text: OnceCell<CosmicShaper>,
}

impl Default for WinitBackend {
    fn default() -> WinitBackend {
        WinitBackend::new()
    }
}

impl WinitBackend {
    /// Creates the backend and its event loop. Must be called on the process's
    /// main thread; the event loop is consumed by
    /// [`Backend::run`](xui_core::backend::Backend::run).
    pub fn new() -> WinitBackend {
        let event_loop = EventLoop::<UserEvent>::with_user_event()
            .build()
            .expect("the winit event loop is created once on the main thread");
        let proxy = event_loop.create_proxy();
        WinitBackend {
            shared: Rc::new(Shared {
                windows: RefCell::new(HashMap::new()),
                nodes: RefCell::new(Scene::new()),
                cursors: RefCell::new(HashMap::new()),
                captured: RefCell::new(None),
                timers: RefCell::new(HashMap::new()),
                next_window: Cell::new(1),
                next_widget: Cell::new(1),
                next_timer: Cell::new(1),
                proxy,
                exit_code: Cell::new(0),
                quit: Cell::new(false),
            }),
            event_loop: RefCell::new(Some(event_loop)),
            text: OnceCell::new(),
        }
    }

    fn allocate(cell: &Cell<u64>) -> u64 {
        let id = cell.get();
        cell.set(id + 1);
        id
    }
}

impl WinitBackend {
    /// Runs `f` with `window`'s live `winit::Window`, if it has one yet.
    fn window_handle<R>(&self, window: WindowId, f: impl FnOnce(&Window) -> R) -> Option<R> {
        let handle = self
            .shared
            .windows
            .borrow()
            .get(&window.raw())
            .and_then(|state| state.window.clone())?;
        Some(f(&handle))
    }

    fn with_node(&self, id: WidgetId, f: impl FnOnce(&mut Node)) {
        if let Some(node) = self.shared.nodes.borrow_mut().get_mut(id) {
            f(node);
        }
    }

    /// Asks for a frame of the window of node `id` when `damaged`: a change
    /// that marked pixels stale is shown without waiting for the next
    /// invalidate.
    fn redraw_if(&self, damaged: bool, id: WidgetId) {
        if damaged && let Some(window) = self.window_of(id) {
            self.shared.request_redraw(window);
        }
    }

    fn window_of(&self, id: WidgetId) -> Option<WindowId> {
        self.shared.nodes.borrow().window_of(id)
    }
}
