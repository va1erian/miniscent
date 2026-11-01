//! The fourth pane: reserved for the 3D view. It shows a placeholder until
//! `set_image` supplies a rendered frame.

use std::cell::RefCell;
use std::rc::Rc;

use xui::Color;
use xui::Image;
use xui::app::Ui;
use xui::backend::{NodeKind, NodeSpec, Result, TextAlign, TextStyle, TextVAlign, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::units::Dip;
use xui::widget::{Control, Placeable};

/// A pane that draws the newest frame given to [`View3d::set_image`].
pub struct View3d<M: 'static> {
    control: Control<M>,
    #[allow(dead_code, reason = "written by set_image, which Play will call")]
    image: Rc<RefCell<Option<Image>>>,
}

impl<M: 'static> View3d<M> {
    pub fn new(ui: &Ui<M>) -> Result<View3d<M>> {
        let control = Control::new(
            ui,
            &NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10)),
        )?;
        let image: Rc<RefCell<Option<Image>>> = Rc::new(RefCell::new(None));
        let shown = image.clone();
        let theme = ui.theme_handle();
        control.set_painter(Rc::new(move |canvas| {
            let theme = theme.get();
            let b = canvas.bounds();
            canvas.clear(theme.input_background.lerp(Color::rgb(0, 0, 0), 0.35));
            if let Some(img) = shown.borrow().as_ref() {
                canvas.draw_image(img, fit(b, img.width(), img.height()));
            } else {
                let mut style = TextStyle::new(theme.text_secondary, Dip(12.0));
                style.align = TextAlign::Center;
                style.valign = TextVAlign::Middle;
                canvas.draw_text("3D view\n(Play renders here)", b, &style);
            }
            canvas.stroke_rect(b, theme.border, 1.0);
        }));
        Ok(View3d { control, image })
    }

    #[allow(
        dead_code,
        reason = "Play will call this each frame; exercised in tests"
    )]
    /// Shows a `0xAARRGGBB` frame of `w` by `h` pixels. Ignored if the slice
    /// is not exactly `w * h` long.
    pub fn set_image(&self, pixels: &[u32], w: u32, h: u32) {
        if w == 0 || h == 0 || pixels.len() != w as usize * h as usize {
            return;
        }
        let mut bytes = Vec::with_capacity(pixels.len() * 4);
        for p in pixels {
            bytes.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255]);
        }
        if let Ok(img) = Image::from_rgba(w, h, bytes) {
            *self.image.borrow_mut() = Some(img);
            self.control.invalidate();
        }
    }
}

/// The largest centred rectangle of the image's aspect that fits `b`.
fn fit(b: Rect, w: u32, h: u32) -> Rect {
    let (bw, bh) = (b.width() as f64, b.height() as f64);
    let s = (bw / w as f64).min(bh / h as f64);
    let (dw, dh) = ((w as f64 * s) as i32, (h as f64 * s) as i32);
    let x = b.left + (b.width() - dw) / 2;
    let y = b.top + (b.height() - dh) / 2;
    Rect::new(x, y, x + dw, y + dh)
}

impl<M: 'static> Placeable<M> for View3d<M> {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, _ui: &Ui<M>, _constraints: Constraints) -> Size {
        Size::new(0, 0)
    }
}
