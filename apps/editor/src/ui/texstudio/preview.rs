//! The preview: one custom-painted node showing the newest picture, once or tiled
//! two by two. Tiling is done here by drawing the same image four times, so a
//! seam shows and no pixels are copied.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::app::Ui;
use xui::backend::{NodeKind, NodeSpec, Result, TextAlign, TextStyle, TextVAlign, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::units::Dip;
use xui::widget::{Control, Placeable};
use xui::{Color, Image};

#[derive(Default)]
struct Shared {
    image: RefCell<Option<Image>>,
    text: RefCell<String>,
    tiled: Cell<bool>,
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

impl<M: 'static> Preview<M> {
    pub fn new(ui: &Ui<M>) -> Result<Preview<M>> {
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
                    for rect in tile_rects(b, image.width(), image.height(), tiles) {
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

    /// The pixels shown (width, height, RGBA bytes), for scripted runs.
    pub fn pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let image = self.shared.image.borrow();
        let image = image.as_ref()?;
        Some((image.width(), image.height(), image.pixels().to_vec()))
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
}
