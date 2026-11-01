#![forbid(unsafe_code)]

//! Real-window creation and per-window DPI: turning the front layer's requested
//! windows into `winit` windows, and keeping their size and node bounds in step
//! with the monitor's scale factor.

use std::rc::Rc;

use winit::dpi::LogicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes};

use xui_core::backend::Decorations;
use xui_core::geometry::Rect;

use super::App;
use crate::backend::software::RealWindow;
use crate::backend::{DEFAULT_DPI, Shared, WindowState};

impl App<'_> {
    /// Creates the OS window for any backend window that lacks one. Its
    /// presentation surface is created lazily on the first frame.
    pub(super) fn create_windows(&mut self, event_loop: &ActiveEventLoop) {
        let ids: Vec<u64> = self.shared.windows.borrow().keys().copied().collect();
        for raw in ids {
            if self.windows.contains_key(&raw) {
                continue;
            }
            let (title, size, decorations, resizable) = {
                let windows = self.shared.windows.borrow();
                let Some(state) = windows.get(&raw) else {
                    continue;
                };
                (
                    state.title.clone(),
                    state.size,
                    state.decorations,
                    state.resizable,
                )
            };
            // A requested backdrop (Acrylic/Mica) is approximated by the opaque
            // theme background on platforms without the DWM material; softbuffer
            // presents an opaque surface, so there is nothing else to do.
            let attributes = window_attributes(title, size, decorations, resizable);
            let Ok(window) = event_loop.create_window(attributes) else {
                continue;
            };
            let window = Rc::new(window);
            if let Some(icon) = self
                .shared
                .windows
                .borrow()
                .get(&raw)
                .and_then(WindowState::winit_icon)
            {
                window.set_window_icon(Some(icon));
            }
            let metrics = window_metrics(&window);
            if let Some(state) = self.shared.windows.borrow_mut().get_mut(&raw) {
                state.size = metrics.0;
                state.window = Some(Rc::clone(&window));
            }
            self.shared
                .nodes
                .borrow_mut()
                .invalidate_window(xui_core::backend::WindowId::from_raw(raw));
            self.set_dpi(raw, metrics.1);
            self.windows.insert(raw, RealWindow::new());
            // Build the primary window's app now that DPI is known, before the
            // first frame, so its widgets lay out at the real scale.
            self.fire_ready(raw);
            self.redraw(raw);
        }
    }

    /// Drops the OS window of any backend window that has been closed.
    ///
    /// The backend's `close_window` removes the window state, but the handler's
    /// [`RealWindow`] holds its own `Rc<winit::Window>` clones (through the
    /// `softbuffer` context and surface), so the OS window survives it. Dropping
    /// the `RealWindow` releases those clones and destroys the window, so a
    /// closed secondary window leaves the screen instead of staying frozen.
    pub(super) fn reconcile_windows(&mut self) {
        let tracked = self.shared.windows.borrow();
        let stale = stale_windows(self.windows.keys().copied(), |raw| {
            tracked.contains_key(&raw)
        });
        drop(tracked);
        for raw in stale {
            self.windows.remove(&raw);
        }
    }

    /// Invokes the deferred app builder for `raw`, if it is waiting for this
    /// window. Exactly once: the builder is taken out before it runs, so the
    /// `create_windows` calls from `user_event`/`about_to_wait` are no-ops.
    fn fire_ready(&mut self, raw: u64) {
        match self.on_ready.as_ref() {
            Some((window, _)) if *window == raw => {}
            _ => return,
        }
        if let Some((_, on_ready)) = self.on_ready.take() {
            on_ready();
        }
    }

    /// Records `raw`'s dots-per-inch, lifting the node bounds the app laid out
    /// at the old value to the new scale so the window stays filled.
    ///
    /// The primary window's app is built after its real window exists (see
    /// [`Backend::run_with`]), so it is already laid out at this DPI and there
    /// are no nodes to lift. A secondary window opened while the loop runs is
    /// still built before its OS window exists, so it is laid out at
    /// [`DEFAULT_DPI`] and its bounds move to the backing pixels here. Later
    /// scale changes (a window dragged to another monitor) rescale by the ratio,
    /// since the portable widgets do not reflow themselves.
    ///
    /// [`Backend::run_with`]: xui_core::backend::Backend::run_with
    pub(super) fn set_dpi(&self, raw: u64, dpi: u32) {
        let old = {
            let mut windows = self.shared.windows.borrow_mut();
            let Some(state) = windows.get_mut(&raw) else {
                return;
            };
            let old = state.dpi;
            state.dpi = dpi;
            old
        };
        if old != dpi {
            rescale_nodes(&self.shared, raw, dpi as f32 / old as f32);
        }
    }
}

/// The `winit` attributes for a backend window: the system title bar is shown
/// only for [`Decorations::System`]; [`Decorations::None`] asks `winit` for an
/// undecorated (frameless) window so the application draws its own caption
/// without the OS chrome doubling it up.
pub(super) fn window_attributes(
    title: String,
    size: (u32, u32),
    decorations: Decorations,
    resizable: bool,
) -> WindowAttributes {
    WindowAttributes::default()
        .with_title(title)
        .with_inner_size(LogicalSize::new(f64::from(size.0), f64::from(size.1)))
        .with_resizable(resizable)
        .with_decorations(decorations == Decorations::System)
}

/// The dots-per-inch a `winit` scale factor corresponds to (96 at 100%).
pub(super) fn dpi_from_scale(scale_factor: f64) -> u32 {
    (scale_factor * f64::from(DEFAULT_DPI)).round().max(1.0) as u32
}

/// The raw ids in `real` whose backend window is absent from `tracked`: the
/// handler holds an OS window for each, so one the backend no longer tracks has
/// been closed and must be destroyed. Pure, so it is tested without an event
/// loop.
fn stale_windows(
    real: impl IntoIterator<Item = u64>,
    mut tracked: impl FnMut(u64) -> bool,
) -> Vec<u64> {
    real.into_iter().filter(|raw| !tracked(*raw)).collect()
}

/// The window's `(size, dpi)` from its scale factor.
fn window_metrics(window: &Window) -> ((u32, u32), u32) {
    let size = window.inner_size();
    let dpi = dpi_from_scale(window.scale_factor());
    ((size.width.max(1), size.height.max(1)), dpi)
}

/// Multiplies `rect`'s edges by `ratio`, rounding to the nearest pixel.
fn scale_rect(rect: Rect, ratio: f32) -> Rect {
    let scale = |value: i32| (value as f32 * ratio).round() as i32;
    Rect::new(
        scale(rect.left),
        scale(rect.top),
        scale(rect.right),
        scale(rect.bottom),
    )
}

/// Rescales every node of `raw` from one layout scale to another.
fn rescale_nodes(shared: &Shared, raw: u64, ratio: f32) {
    if (ratio - 1.0).abs() < f32::EPSILON {
        return;
    }
    let mut nodes = shared.nodes.borrow_mut();
    for node in nodes.nodes_mut() {
        if node.window.raw() == raw {
            node.bounds = scale_rect(node.bounds, ratio);
        }
    }
    // Every node moved: the whole window is stale.
    nodes.invalidate_window(xui_core::backend::WindowId::from_raw(raw));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_factor_maps_to_a_dots_per_inch() {
        assert_eq!(dpi_from_scale(1.0), 96);
        assert_eq!(dpi_from_scale(1.25), 120);
        assert_eq!(dpi_from_scale(2.0), 192, "a Retina display is 2x");
        assert_eq!(dpi_from_scale(0.0), 1, "never zero");
    }

    #[test]
    fn scaling_a_rect_lifts_the_backing_store() {
        let rect = Rect::new(16, 12, 380, 40);
        assert_eq!(scale_rect(rect, 2.0), Rect::new(32, 24, 760, 80));
        assert_eq!(scale_rect(rect, 1.25), Rect::new(20, 15, 475, 50));
    }

    #[test]
    fn a_backend_window_that_is_gone_is_stale() {
        let live = [1u64, 3];
        let stale = stale_windows([1, 2, 3], |raw| live.contains(&raw));
        assert_eq!(stale, [2], "the closed secondary window is dropped");
    }

    #[test]
    fn windows_the_backend_still_tracks_are_kept() {
        assert!(stale_windows([1, 2], |_| true).is_empty());
    }

    #[test]
    fn a_handler_window_with_no_backend_state_is_stale() {
        assert_eq!(stale_windows([7], |_| false), [7]);
    }

    #[test]
    fn only_a_system_decorations_spec_asks_for_the_os_frame() {
        let decorated = window_attributes("t".into(), (200, 100), Decorations::System, true);
        assert!(
            decorated.decorations,
            "the default keeps the system title bar"
        );

        let borderless = window_attributes("t".into(), (200, 100), Decorations::None, true);
        assert!(
            !borderless.decorations,
            "Decorations::None must not request the OS title bar"
        );
        assert!(borderless.resizable);
    }
}
