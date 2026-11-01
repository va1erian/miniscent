#![forbid(unsafe_code)]

//! The GL content seam on [`WinitBackend`]: installing and removing a
//! [`GlWidget`] for a window or for a single node, and scheduling its repaints.
//!
//! Window-level content is the base layer of the whole client area; node-level
//! content is composited at that node's bounds, among the window's other nodes,
//! so a GL visualizer can sit in one pane of an ordinary app.

use std::rc::Rc;

use xui_core::backend::{WidgetId, WindowId};

use super::WinitBackend;
use crate::gl::GlWidget;

impl WinitBackend {
    /// Installs `widget` as `window`'s GPU-rendered content.
    ///
    /// From the next repaint on, the backend renders [`GlWidget::paint_gl`]
    /// frames through a [`GlSurface`](crate::GlSurface) into an offscreen
    /// texture and composites them through the software painter model, at the
    /// whole client area, beneath the window's ordinary nodes. When no GL
    /// context can be created, or a frame cannot be rendered, the backend falls
    /// back to [`GlWidget::paint`] on the software surface for good.
    pub fn set_gl_content<W: GlWidget + 'static>(&self, window: WindowId, widget: W) {
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            let previous = state.gl.replace(Rc::new(widget));
            // Free the replaced widget's GPU objects with the context current,
            // keeping the surface for any node-level content.
            if let Some(previous) = previous {
                state.renderer.with_gl(|gl| previous.gl_teardown(gl));
            }
        }
        self.shared.invalidate_window(window);
    }

    /// Removes `window`'s window-level GPU content, releasing its GPU resources
    /// with the context current. The window returns to the software path for any
    /// nodes without their own GL content.
    pub fn clear_gl_content(&self, window: WindowId) {
        let removed = self
            .shared
            .windows
            .borrow_mut()
            .get_mut(&window.raw())
            .and_then(|state| state.gl.take());
        if let Some(widget) = removed
            && let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw())
        {
            state.renderer.with_gl(|gl| widget.gl_teardown(gl));
        }
        self.shared.invalidate_window(window);
    }

    /// Installs `widget` as the GPU-rendered content of the node `id`.
    ///
    /// Unlike [`set_gl_content`](WinitBackend::set_gl_content), which covers the
    /// whole client area, this renders at the node's bounds and composites among
    /// the window's other nodes, so GL content can sit in one pane. The node is
    /// found on whichever window owns it; a missing node is ignored.
    pub fn set_gl_content_on<W: GlWidget + 'static>(&self, id: WidgetId, widget: W) {
        let Some(window) = self.window_of(id) else {
            return;
        };
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            let previous = state.gl_nodes.insert(id, Rc::new(widget));
            if let Some(previous) = previous {
                state.renderer.with_gl(|gl| previous.gl_teardown(gl));
            }
        }
        self.shared.invalidate_window(window);
    }

    /// Removes the GPU content of node `id`, releasing its GPU resources with
    /// the context current.
    pub fn clear_gl_content_on(&self, id: WidgetId) {
        let Some(window) = self.window_of(id) else {
            return;
        };
        let removed = self
            .shared
            .windows
            .borrow_mut()
            .get_mut(&window.raw())
            .and_then(|state| state.gl_nodes.remove(&id));
        if let Some(widget) = removed
            && let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw())
        {
            state.renderer.with_gl(|gl| widget.gl_teardown(gl));
        }
        self.shared.invalidate_window(window);
    }

    /// Runs `window`'s GL widgets' teardown with its context current, if it has
    /// one. Called before the window is destroyed, while its surface is still
    /// live.
    pub(crate) fn teardown_window_gl(&self, window: WindowId) {
        let widgets: Vec<Rc<dyn GlWidget>> = {
            let windows = self.shared.windows.borrow();
            let Some(state) = windows.get(&window.raw()) else {
                return;
            };
            state
                .gl
                .iter()
                .chain(state.gl_nodes.values())
                .cloned()
                .collect()
        };
        if let Some(state) = self.shared.windows.borrow_mut().get_mut(&window.raw()) {
            state
                .renderer
                .teardown_gl(|gl| widgets.iter().for_each(|widget| widget.gl_teardown(gl)));
        }
    }

    /// Schedules a repaint of `window`. Use it for window-level GL content,
    /// which has no node to invalidate.
    pub fn request_redraw(&self, window: WindowId) {
        self.shared.invalidate_window(window);
    }
}
