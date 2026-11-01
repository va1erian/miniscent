#![forbid(unsafe_code)]

//! Compositing a window's nodes into a [`Surface`] with the shared compositor,
//! and the windowed backend's GL content that it lends the compositor.

use std::rc::Rc;

use xui_core::Theme;
use xui_core::backend::{Canvas as _, WidgetId, WindowId};
use xui_core::geometry::Rect;
use xui_core::image::Image;

use super::Shared;
use crate::Surface;
use crate::compositor::{self, Content, FrameInfo};
use crate::gl::GlWidget;

/// Where a window's GL content is attached.
#[derive(Clone, Copy)]
enum GlSlot {
    /// Covers the whole client area, as the base layer.
    Window,
    /// Covers one node's bounds, composited among the window's other nodes.
    Node(WidgetId),
}

/// A window's GL content, lent to the compositor.
///
/// GL content is one painter among many: each frame is rendered into a
/// texture, read back as pixels and drawn through the same software surface,
/// so CPU nodes composite over it instead of being hidden by a GPU frame that
/// took over the whole window; node-level content sits in its pane. When no GL
/// context or framebuffer is available the widget's software fallback is
/// painted instead, so the window stays usable.
pub(super) struct WindowGl<'a> {
    pub(super) shared: &'a Shared,
    pub(super) window: WindowId,
}

impl Content for WindowGl<'_> {
    fn has_node(&self, id: WidgetId) -> bool {
        self.shared
            .windows
            .borrow()
            .get(&self.window.raw())
            .is_some_and(|state| state.gl_nodes.contains_key(&id))
    }

    fn is_live(&self) -> bool {
        self.shared
            .windows
            .borrow()
            .get(&self.window.raw())
            .is_some_and(|state| state.gl.is_some() || !state.gl_nodes.is_empty())
    }

    fn paint_window(&self, region: &mut crate::Region<'_>, window: Rect, dpi: u32, theme: &Theme) {
        draw_gl(
            self.shared,
            self.window,
            GlSlot::Window,
            window,
            None,
            theme,
            dpi,
            region,
        );
    }

    fn paint_node(
        &self,
        region: &mut crate::Region<'_>,
        id: WidgetId,
        bounds: Rect,
        clip: Option<Rect>,
        dpi: u32,
        theme: &Theme,
    ) {
        draw_gl(
            self.shared,
            self.window,
            GlSlot::Node(id),
            bounds,
            clip,
            theme,
            dpi,
            region,
        );
    }
}

/// Fills `surface` with the window's background, composites the window's
/// window-level GL content as the base layer, then runs each visible node's
/// painter at its absolute bounds, in creation order (later nodes on top),
/// clipped to its ancestors' clips, with per-node GL content at that node's
/// bounds: the whole window, through the shared compositor.
pub(super) fn composite(shared: &Shared, window: WindowId, surface: &mut Surface) {
    let (theme, dpi, size) = {
        let windows = shared.windows.borrow();
        let Some(state) = windows.get(&window.raw()) else {
            return;
        };
        (state.theme, state.dpi, state.size)
    };
    let window_rect = Rect::new(0, 0, size.0 as i32, size.1 as i32);
    let content = WindowGl { shared, window };
    let plan = compositor::plan(&shared.nodes.borrow(), window, dpi, &content);
    let frame = FrameInfo {
        window: window_rect,
        dpi,
        theme: &theme,
    };
    plan.paint(surface, &[window_rect], &frame, &content);
}

/// Renders the GL content at `slot` into an image of `bounds`' size and draws
/// it at `bounds`. When no GL context or framebuffer is available the widget's
/// software fallback is painted instead, so the window stays usable.
#[allow(clippy::too_many_arguments)]
fn draw_gl(
    shared: &Shared,
    window: WindowId,
    slot: GlSlot,
    bounds: Rect,
    clip: Option<Rect>,
    theme: &Theme,
    dpi: u32,
    region: &mut crate::Region<'_>,
) {
    if bounds.is_empty() {
        return;
    }
    let Some(widget) = gl_widget(shared, window, slot) else {
        return;
    };
    let image = render_image(shared, window, &widget, bounds, theme);
    region.with_canvas_at(bounds, dpi, |canvas| {
        if let Some(clip) = clip {
            canvas.push_clip(clip);
        }
        match &image {
            Some(image) => canvas.draw_image(image, bounds),
            None => widget.paint(canvas, bounds, theme),
        }
    });
}

/// The GL widget attached at `slot`, if any.
fn gl_widget(shared: &Shared, window: WindowId, slot: GlSlot) -> Option<Rc<dyn GlWidget>> {
    let windows = shared.windows.borrow();
    let state = windows.get(&window.raw())?;
    match slot {
        GlSlot::Window => state.gl.clone(),
        GlSlot::Node(id) => state.gl_nodes.get(&id).cloned(),
    }
}

/// Renders `widget` into an offscreen image of `bounds`' size, or `None` when no
/// GL context or framebuffer is available.
fn render_image(
    shared: &Shared,
    window: WindowId,
    widget: &Rc<dyn GlWidget>,
    bounds: Rect,
    theme: &Theme,
) -> Option<Image> {
    let mut windows = shared.windows.borrow_mut();
    let state = windows.get_mut(&window.raw())?;
    let handle = state.window.clone()?;
    state.renderer.render(
        &handle,
        bounds.width() as u32,
        bounds.height() as u32,
        theme.background,
        |gl| widget.paint_gl(gl, bounds, theme),
        |gl| widget.gl_teardown(gl),
    )
}
