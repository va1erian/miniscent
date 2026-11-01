#![forbid(unsafe_code)]

//! The compositor both canvas backends share: what to paint, in which order,
//! and how, for a window of a [`Scene`].
//!
//! A frame is two steps so that no borrow of the scene is held while painters
//! run (a painter may call back into the backend):
//!
//! 1. [`plan`] walks the window's nodes once, in creation order, and keeps the
//!    ones that paint: visible through their whole ancestry, with something
//!    left inside their ancestors' clips. Each carries its window-absolute
//!    bounds, its clip, and its *reach*: its bounds trimmed to the clips,
//!    grown by the overdraw margin.
//! 2. [`Plan::paint`] repaints rectangles of a [`Surface`]: for each, the
//!    window background, then every planned node whose reach meets it, in
//!    creation order, clipped to its ancestors' clips **and to its reach**.
//!
//! A painter cannot draw outside its reach, whether the frame repaints the
//! whole window or one damaged rectangle: the clip is the same in both, which
//! is what makes a repaint of damage equal a repaint of everything for any
//! painter, and what damage tracking relies on (a node changes only pixels
//! inside its reach, so only those need repainting). A widget that has to draw
//! further out than the margin has to enlarge its node's bounds.
//!
//! Painting a rectangle is a [`Surface::paint_region`], so the pixels inside it
//! are exactly what a whole-window paint draws there and the rest are not
//! touched. The windowed backend and the offscreen backend call the same two
//! functions, so what they draw cannot drift.
//!
//! GL content (the windowed backend's renderer, the offscreen backend's
//! software fallback) is a [`Content`] the backend lends the compositor.

use std::collections::HashMap;

use xui_core::Theme;
use xui_core::backend::{Canvas as _, Painter, ParentRef, WidgetId, WindowId};
use xui_core::geometry::Rect;

use crate::geometry::{Placement, intersect};
use crate::scene::Scene;
use crate::{Region, Surface};

/// How far a painter's pixels may reach outside its node's bounds, in design
/// units (pixels at 96 DPI): the overdraw margin.
///
/// Strokes are centred on a shape's edge, so a 1 px outline on the bounds'
/// edge covers half a pixel outside it and anti-aliasing shows it; a focus
/// ring or a glow (a theme's accent halo reaches about three pixels) goes
/// further, and a ring drawn in design units grows with the DPI, so the
/// margin does too ([`margin`]). The measured need on the in-tree widgets is
/// 1 px on the flat themes and 6 px on Midnight at 96 DPI.
///
/// A node's *reach* is its bounds, trimmed to its ancestors' clips and grown
/// by the margin. The compositor clips each painter to its reach, so nothing is
/// drawn outside it; damage is grown by the same margin, and a damaged
/// rectangle repaints every node whose reach touches it.
pub(crate) const OVERDRAW_DIP: i32 = 8;

/// The overdraw margin at `dpi`, in device pixels: [`OVERDRAW_DIP`] design
/// units, rounded up.
pub(crate) fn margin(dpi: u32) -> i32 {
    let scaled = (u64::from(OVERDRAW_DIP.unsigned_abs()) * u64::from(dpi)).div_ceil(96);
    i32::try_from(scaled).unwrap_or(i32::MAX).min(EDGE_LIMIT)
}

/// The most any edge of a damaged or reached rectangle can be from the origin:
/// far outside any window, and small enough that sums and widths of such
/// rectangles cannot overflow even where a layout saturated at `i32::MAX`.
const EDGE_LIMIT: i32 = 1 << 29;

/// `rect` grown by `by` pixels on every side, its edges kept within
/// [`EDGE_LIMIT`]; an empty rectangle stays empty.
pub(crate) fn grow(rect: Rect, by: i32) -> Rect {
    if rect.is_empty() {
        return rect;
    }
    let edge = |value: i32| value.clamp(-EDGE_LIMIT, EDGE_LIMIT);
    Rect::new(
        edge(rect.left.saturating_sub(by)),
        edge(rect.top.saturating_sub(by)),
        edge(rect.right.saturating_add(by)),
        edge(rect.bottom.saturating_add(by)),
    )
}

/// What the compositor lends a backend's extra content: GPU-rendered widgets,
/// or their software fallback. Everything it paints goes through the same
/// region as ordinary nodes.
pub(crate) trait Content {
    /// Whether node `id` carries content of its own, so it is planned even
    /// without a painter.
    fn has_node(&self, id: WidgetId) -> bool;

    /// Whether the window's content changes without the compositor being told
    /// (a GL frame; a software fallback reading a live value). A live window
    /// is repainted whole every frame.
    fn is_live(&self) -> bool;

    /// Paints the window-level content as the base layer, over `window`.
    fn paint_window(&self, region: &mut Region<'_>, window: Rect, dpi: u32, theme: &Theme);

    /// Paints node `id`'s content at its absolute `bounds`, under `clip`.
    fn paint_node(
        &self,
        region: &mut Region<'_>,
        id: WidgetId,
        bounds: Rect,
        clip: Option<Rect>,
        dpi: u32,
        theme: &Theme,
    );
}

/// One node to draw.
struct Draw {
    id: WidgetId,
    /// Window-absolute bounds.
    bounds: Rect,
    /// The clip its ancestors impose, in window coordinates.
    clip: Option<Rect>,
    /// Where its pixels can land: its bounds trimmed to the clip, grown by
    /// the margin.
    reach: Rect,
    painter: Option<Painter>,
    /// Whether `Content` also paints at this node.
    content: bool,
}

/// The nodes of one window that paint, in creation order.
pub(crate) struct Plan {
    draws: Vec<Draw>,
}

/// Resolves `id`'s placement, and its ancestors' on the way, into `resolved`.
/// `chain` is scratch space for the walk up.
fn place(
    scene: &Scene,
    id: WidgetId,
    resolved: &mut HashMap<WidgetId, Placement>,
    chain: &mut Vec<WidgetId>,
) -> Option<Placement> {
    if let Some(placement) = resolved.get(&id) {
        return Some(*placement);
    }
    chain.clear();
    let mut above: Option<Placement> = None;
    let mut current = id;
    loop {
        let node = scene.get(current)?;
        chain.push(current);
        match node.parent {
            ParentRef::Window(_) => break,
            ParentRef::Widget(parent) => {
                if let Some(placement) = resolved.get(&parent) {
                    above = Some(*placement);
                    break;
                }
                current = parent;
            }
        }
    }
    for member in chain.iter().rev() {
        let placement = Placement::of(scene.get(*member)?, above.as_ref());
        resolved.insert(*member, placement);
        above = Some(placement);
    }
    above
}

/// Plans a frame of `window`: its painting nodes in creation order, with
/// bounds and clips resolved in a single pass over the scene.
pub(crate) fn plan(scene: &Scene, window: WindowId, dpi: u32, content: &dyn Content) -> Plan {
    let overdraw = margin(dpi);
    let mut resolved = HashMap::new();
    let mut chain = Vec::new();
    let mut draws = Vec::new();
    for (id, node) in scene.iter() {
        if node.window != window {
            continue;
        }
        let has_content = content.has_node(id);
        if node.painter.is_none() && !has_content {
            continue;
        }
        let Some(placement) = place(scene, id, &mut resolved, &mut chain) else {
            continue;
        };
        if !placement.visible {
            continue;
        }
        // A node the clips leave nothing of is not drawn; one with no area of
        // its own and no clip still is (its painter may draw around it).
        if let Some(clip) = placement.clip
            && intersect(placement.bounds, clip).is_empty()
        {
            continue;
        }
        let reach = placement.visible_bounds();
        draws.push(Draw {
            id,
            bounds: placement.bounds,
            clip: placement.clip,
            reach: grow(reach, overdraw),
            painter: node.painter.clone(),
            content: has_content,
        });
    }
    Plan { draws }
}

/// What painting a frame needs besides the plan.
pub(crate) struct FrameInfo<'a> {
    /// The window's rectangle, `(0, 0, width, height)`.
    pub(crate) window: Rect,
    /// Dots per inch the painters scale their design values by.
    pub(crate) dpi: u32,
    /// The theme the background reads.
    pub(crate) theme: &'a Theme,
}

impl Plan {
    /// How many nodes the plan paints.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.draws.len()
    }

    /// The ids of the planned nodes, in paint order.
    #[cfg(test)]
    pub(crate) fn ids(&self) -> Vec<WidgetId> {
        self.draws.iter().map(|draw| draw.id).collect()
    }

    /// Repaints each of `areas` (window coordinates, inside the window) of
    /// `surface`: the window background, then the planned nodes whose reach
    /// meets the area, in creation order.
    ///
    /// Every pixel inside an area ends up as a whole-window paint draws it;
    /// pixels outside are not touched.
    pub(crate) fn paint(
        &self,
        surface: &mut Surface,
        areas: &[Rect],
        frame: &FrameInfo<'_>,
        content: &dyn Content,
    ) {
        for area in areas {
            surface.paint_region(*area, |region| {
                region.with_canvas_at(frame.window, frame.dpi, |canvas| {
                    xui_core::theme::look::paint_background(
                        canvas,
                        *area,
                        frame.window,
                        frame.theme,
                    );
                });
                content.paint_window(region, frame.window, frame.dpi, frame.theme);
                for draw in &self.draws {
                    if intersect(draw.reach, *area).is_empty() {
                        continue;
                    }
                    // Nodes paint in creation order, so a widget's container
                    // is already on the surface when the widget paints over it.
                    if let Some(painter) = &draw.painter {
                        region.with_canvas_over_parents(draw.bounds, frame.dpi, |canvas| {
                            if let Some(clip) = draw.clip {
                                canvas.push_clip(clip);
                            }
                            // Nothing is drawn outside the reach, in a frame
                            // of the whole window or of a damaged rectangle.
                            canvas.reach = Some(draw.reach);
                            painter(canvas);
                        });
                    }
                    if draw.content {
                        content.paint_node(
                            region,
                            draw.id,
                            draw.bounds,
                            draw.clip,
                            frame.dpi,
                            frame.theme,
                        );
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests;
