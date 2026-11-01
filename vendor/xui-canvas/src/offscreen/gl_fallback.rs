#![forbid(unsafe_code)]

//! GL content installed on an offscreen window, painted through its software
//! fallback. The offscreen backend never has a GL context, so a `GlWidget`
//! installed here is the same seam the windowed backend exposes, exercised
//! headlessly.
//!
//! With the `winit-backend` feature off there is no `GlWidget` seam at all, so
//! this is a zero-sized placeholder whose content paints nothing; the offscreen
//! painter then compiles without the windowed modules.

#[cfg(feature = "winit-backend")]
use std::collections::HashMap;
#[cfg(feature = "winit-backend")]
use std::rc::Rc;

#[cfg(feature = "winit-backend")]
use xui_core::backend::Canvas as _;
use xui_core::backend::WidgetId;
use xui_core::{Rect, Theme};

use crate::Region;
use crate::compositor::Content;
#[cfg(feature = "winit-backend")]
use crate::gl::GlWidget;

#[cfg(feature = "winit-backend")]
pub(super) struct GlContent {
    window: Option<Rc<dyn GlWidget>>,
    nodes: HashMap<u64, Rc<dyn GlWidget>>,
}

#[cfg(feature = "winit-backend")]
impl GlContent {
    /// Empty GL content.
    pub(super) fn new() -> GlContent {
        GlContent {
            window: None,
            nodes: HashMap::new(),
        }
    }

    /// Installs `widget` as the window-level content.
    pub(super) fn set_window<W: GlWidget + 'static>(&mut self, widget: W) {
        self.window = Some(Rc::new(widget));
    }

    /// Removes the window-level content.
    pub(super) fn clear_window(&mut self) {
        self.window = None;
    }

    /// Installs `widget` as the content of node `id`.
    pub(super) fn set_node<W: GlWidget + 'static>(&mut self, id: WidgetId, widget: W) {
        self.nodes.insert(id.raw(), Rc::new(widget));
    }

    /// Removes the content of node `id`.
    pub(super) fn clear_node(&mut self, id: WidgetId) {
        self.nodes.remove(&id.raw());
    }

    /// A snapshot of the content, taken before painting so a widget's fallback
    /// paint can re-enter the backend without the window map staying borrowed.
    pub(super) fn snapshot(&self) -> GlSnapshot {
        GlSnapshot {
            window: self.window.clone(),
            nodes: self
                .nodes
                .iter()
                .map(|(raw, widget)| (*raw, Rc::clone(widget)))
                .collect(),
        }
    }
}

/// A cloned view of a window's GL content, safe to paint after the window map
/// borrow is released.
#[cfg(feature = "winit-backend")]
pub(super) struct GlSnapshot {
    window: Option<Rc<dyn GlWidget>>,
    nodes: Vec<(u64, Rc<dyn GlWidget>)>,
}

#[cfg(feature = "winit-backend")]
impl Content for GlSnapshot {
    fn has_node(&self, id: WidgetId) -> bool {
        self.nodes.iter().any(|(raw, _)| *raw == id.raw())
    }

    fn is_live(&self) -> bool {
        self.window.is_some() || !self.nodes.is_empty()
    }

    /// Paints the window-level widget's software fallback as the base layer.
    fn paint_window(&self, region: &mut Region<'_>, window: Rect, dpi: u32, theme: &Theme) {
        if let Some(widget) = &self.window {
            region.with_canvas_at(window, dpi, |canvas| widget.paint(canvas, window, theme));
        }
    }

    /// Paints node `id`'s software fallback at `bounds`, clipped to `clip`.
    fn paint_node(
        &self,
        region: &mut Region<'_>,
        id: WidgetId,
        bounds: Rect,
        clip: Option<Rect>,
        dpi: u32,
        theme: &Theme,
    ) {
        let Some((_, widget)) = self.nodes.iter().find(|(raw, _)| *raw == id.raw()) else {
            return;
        };
        region.with_canvas_at(bounds, dpi, |canvas| {
            if let Some(clip) = clip {
                canvas.push_clip(clip);
            }
            widget.paint(canvas, bounds, theme);
        });
    }
}

/// With the windowed backend off there is no `GlWidget` seam, so an offscreen
/// window carries no GL content.
#[cfg(not(feature = "winit-backend"))]
pub(super) struct GlContent;

#[cfg(not(feature = "winit-backend"))]
impl GlContent {
    /// Empty GL content.
    pub(super) fn new() -> GlContent {
        GlContent
    }

    /// A snapshot that never reports content.
    pub(super) fn snapshot(&self) -> GlSnapshot {
        GlSnapshot
    }
}

/// A no-op snapshot when the GL seam is compiled out.
#[cfg(not(feature = "winit-backend"))]
pub(super) struct GlSnapshot;

#[cfg(not(feature = "winit-backend"))]
impl Content for GlSnapshot {
    fn has_node(&self, _id: WidgetId) -> bool {
        false
    }
    fn is_live(&self) -> bool {
        false
    }

    fn paint_window(&self, _region: &mut Region<'_>, _window: Rect, _dpi: u32, _theme: &Theme) {}

    fn paint_node(
        &self,
        _region: &mut Region<'_>,
        _id: WidgetId,
        _bounds: Rect,
        _clip: Option<Rect>,
        _dpi: u32,
        _theme: &Theme,
    ) {
    }
}
