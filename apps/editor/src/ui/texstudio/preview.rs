//! The preview: one custom-painted node showing the newest picture, once or tiled
//! two by two. Tiling is done here by drawing the same image four times, so a
//! seam shows and no pixels are copied. When the selected block can be painted on,
//! a drag over the picture is reported in tiles of it, for the paint tool.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::app::Ui;
use xui::backend::{Event, NodeKind, NodeSpec, Result, TextAlign, TextStyle, TextVAlign, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::units::Dip;
use xui::widget::{Control, Placeable};
use xui::{Color, Image, MouseButton};

use crate::texstudio::Stroking;

#[derive(Default)]
struct Shared {
    image: RefCell<Option<Image>>,
    text: RefCell<String>,
    tiled: Cell<bool>,
    /// A drag draws, as the page's selection and tab allow.
    paintable: Cell<bool>,
    /// Where the pointer last was, in tiles, while a stroke is being drawn.
    stroking: Cell<Option<[f32; 2]>>,
    /// The first tile as last painted, in the window's pixels.
    first_tile: Cell<Rect>,
}

/// A pane that draws a tile of pixels.
pub struct Preview<M: 'static> {
    control: Control<M>,
    shared: Rc<Shared>,
}

/// The rectangles to draw `tiles` by `tiles` copies of a `w` by `h` picture in, as
/// large as fits and centred.
fn tile_rects(b: Rect, w: u32, h: u32, tiles: i32) -> Vec<Rect> {
    let (bw, bh) = (f64::from(b.width()), f64::from(b.height()));
    let (tw, th) = (
        f64::from(w) * f64::from(tiles),
        f64::from(h) * f64::from(tiles),
    );
    let scale = (bw / tw).min(bh / th);
    let (dw, dh) = ((f64::from(w) * scale) as i32, (f64::from(h) * scale) as i32);
    let (left, top) = (
        b.left + (b.width() - dw * tiles) / 2,
        b.top + (b.height() - dh * tiles) / 2,
    );
    let mut rects = Vec::new();
    for y in 0..tiles {
        for x in 0..tiles {
            let (l, t) = (left + x * dw, top + y * dh);
            rects.push(Rect::new(l, t, l + dw, t + dh));
        }
    }
    rects
}

/// A point of the node in tiles from the picture's top left corner, clamped to the
/// picture; `None` for a picture with no area. `inside` says whether it was over
/// the picture.
fn in_tiles(
    node: Rect,
    picture: (u32, u32),
    tiles: i32,
    (x, y): (i32, i32),
) -> Option<([f32; 2], bool)> {
    let rects = tile_rects(node, picture.0, picture.1, tiles);
    let first = rects.first()?;
    let (dw, dh) = (first.width(), first.height());
    if dw <= 0 || dh <= 0 {
        return None;
    }
    let (u, v) = (
        (x - first.left) as f32 / dw as f32,
        (y - first.top) as f32 / dh as f32,
    );
    let span = tiles as f32;
    let inside = (0.0..span).contains(&u) && (0.0..span).contains(&v);
    Some(([u.clamp(0.0, span), v.clamp(0.0, span)], inside))
}

impl<M: 'static> Preview<M> {
    /// `stroke` makes the message for the pointer drawing on the picture.
    pub fn new(ui: &Ui<M>, stroke: impl Fn(Stroking) -> M + 'static) -> Result<Preview<M>> {
        let control = Control::new(
            ui,
            &NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10)),
        )?;
        let shared = Rc::new(Shared::default());
        {
            let shared = shared.clone();
            let theme = ui.theme_handle();
            control.set_painter(Rc::new(move |canvas| {
                let theme = theme.get();
                let b = canvas.bounds();
                canvas.clear(theme.input_background.lerp(Color::rgb(0, 0, 0), 0.35));
                if let Some(image) = shared.image.borrow().as_ref() {
                    let tiles = if shared.tiled.get() { 2 } else { 1 };
                    for (i, rect) in tile_rects(b, image.width(), image.height(), tiles)
                        .into_iter()
                        .enumerate()
                    {
                        if i == 0 {
                            shared.first_tile.set(rect);
                        }
                        canvas.draw_image(image, rect);
                    }
                } else {
                    let mut style = TextStyle::new(theme.text_secondary, Dip(12.0));
                    style.align = TextAlign::Center;
                    style.valign = TextVAlign::Middle;
                    canvas.draw_text(shared.text.borrow().as_str(), b, &style);
                }
                canvas.stroke_rect(b, theme.border, 1.0);
            }));
        }
        {
            let shared = shared.clone();
            let ui = ui.clone();
            let id = control.id();
            control.on_events(move |event| {
                let node = ui.bounds(id);
                let node = Rect::new(0, 0, node.width(), node.height());
                pointer(
                    &Pointer {
                        ui: &ui,
                        id,
                        node,
                        shared: &shared,
                    },
                    event,
                )
                .map(&stroke)
            });
        }
        Ok(Preview { control, shared })
    }

    /// Shows a picture; the bytes are moved in, not copied. A wrong length changes
    /// nothing.
    pub fn set_rgba(&self, w: u32, h: u32, rgba: Vec<u8>) {
        if let Ok(image) = Image::from_rgba(w, h, rgba) {
            *self.shared.image.borrow_mut() = Some(image);
            self.control.invalidate();
        }
    }

    /// Replaces the picture with a line of text.
    pub fn show_text(&self, text: &str) {
        *self.shared.image.borrow_mut() = None;
        *self.shared.text.borrow_mut() = text.to_string();
        self.control.invalidate();
    }

    pub fn set_tiled(&self, tiled: bool) {
        if self.shared.tiled.replace(tiled) != tiled {
            self.control.invalidate();
        }
    }

    /// Whether a drag over the picture draws.
    pub fn set_paintable(&self, paintable: bool) {
        self.shared.paintable.set(paintable);
    }

    /// The pixels shown (width, height, RGBA bytes), for scripted runs.
    pub fn pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let image = self.shared.image.borrow();
        let image = image.as_ref()?;
        Some((image.width(), image.height(), image.pixels().to_vec()))
    }

    /// The text shown when there is no picture, for scripted runs.
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.shared.text.borrow().clone()
    }

    /// The window's pixel at `u`, `v` tiles from the picture's top left corner as
    /// last painted, for scripted runs to point at.
    pub fn pixel_at(&self, u: f32, v: f32) -> (i32, i32) {
        let tile = self.shared.first_tile.get();
        (
            tile.left + (u * tile.width() as f32).round() as i32,
            tile.top + (v * tile.height() as f32).round() as i32,
        )
    }
}

/// What a pointer event needs.
struct Pointer<'a, M: 'static> {
    ui: &'a Ui<M>,
    id: WidgetId,
    /// The node, from its own top left corner: where events are.
    node: Rect,
    shared: &'a Shared,
}

/// The paint tool's view of one event: a press on the picture starts a stroke, the
/// pointer then reports where it goes, and letting go (or losing the pointer)
/// ends it.
fn pointer<M: 'static>(p: &Pointer<'_, M>, event: &Event) -> Option<Stroking> {
    let shared = p.shared;
    let to_tiles = |x: i32, y: i32| {
        let image = shared.image.borrow();
        let image = image.as_ref()?;
        let tiles = if shared.tiled.get() { 2 } else { 1 };
        in_tiles(p.node, (image.width(), image.height()), tiles, (x, y))
    };
    match event {
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            ..
        } if shared.paintable.get() => {
            let (at, inside) = to_tiles(*x, *y)?;
            if !inside {
                return None;
            }
            p.ui.set_capture(p.id);
            shared.stroking.set(Some(at));
            Some(Stroking::Down(at))
        }
        Event::MouseMove { x, y, .. } if shared.stroking.get().is_some() => {
            let (at, _) = to_tiles(*x, *y)?;
            shared.stroking.set(Some(at));
            Some(Stroking::Move(at))
        }
        Event::MouseUp {
            button: MouseButton::Left,
            ..
        } => {
            let at = shared.stroking.take()?;
            p.ui.release_capture();
            Some(Stroking::Up(at))
        }
        // The pointer was taken away: the stroke ends where it was.
        Event::CaptureChanged => shared.stroking.take().map(Stroking::Up),
        _ => None,
    }
}

impl<M: 'static> Placeable<M> for Preview<M> {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, _ui: &Ui<M>, _constraints: Constraints) -> Size {
        Size::new(0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_fit_are_centred_and_touch() {
        let b = Rect::new(0, 0, 400, 200);
        let one = tile_rects(b, 128, 128, 1);
        assert_eq!(one, [Rect::new(100, 0, 300, 200)]);
        let four = tile_rects(b, 64, 64, 2);
        assert_eq!(four.len(), 4);
        assert_eq!(four[0], Rect::new(100, 0, 200, 100));
        assert_eq!(four[3], Rect::new(200, 100, 300, 200));
    }

    #[test]
    fn a_point_is_in_tiles_of_the_picture_and_clamped_to_it() {
        let node = Rect::new(0, 0, 400, 200);
        let at = |tiles, x, y| in_tiles(node, (128, 128), tiles, (x, y));
        assert_eq!(at(1, 100, 0), Some(([0.0, 0.0], true)));
        assert_eq!(at(1, 200, 100), Some(([0.5, 0.5], true)));
        assert_eq!(at(1, 0, 100), Some(([0.0, 0.5], false)), "beside it");
        assert_eq!(at(1, 399, 500), Some(([1.0, 1.0], false)), "past it");
        // Two tiles across the same space: a tile is half as wide.
        assert_eq!(at(2, 200, 100), Some(([1.0, 1.0], true)));
        assert_eq!(at(2, 250, 50), Some(([1.5, 0.5], true)));
        assert_eq!(in_tiles(Rect::new(0, 0, 0, 0), (128, 128), 1, (0, 0)), None);
    }
}
