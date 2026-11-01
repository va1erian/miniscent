//! A square icon-only toggle button with a tooltip: the tool palette's
//! buttons. The selected one is filled with the theme's accent colour.

use std::cell::Cell;
use std::rc::Rc;

use xui::app::Ui;
use xui::backend::{Event, NodeKind, NodeSpec, Result, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::widget::{Control, Placeable, Tooltip};
use xui::{Key, MouseButton};

use super::icons::{Glyph, draw};

/// Side of a button, in dip.
const SIDE: i32 = 34;
/// Side of the icon inside it, in dip.
const ICON: i32 = 20;

/// One tool button. Clicking (or Space/Enter while focused) raises `msg`;
/// the app owns which button is checked and calls [`IconButton::set_checked`].
pub struct IconButton<M: 'static> {
    control: Control<M>,
    checked: Rc<Cell<bool>>,
    _tip: Tooltip<M>,
}

impl<M: Clone + 'static> IconButton<M> {
    pub fn new(ui: &Ui<M>, glyph: Glyph, tip: &str, msg: M) -> Result<IconButton<M>> {
        let spec = NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, SIDE, SIDE)).tab_stop();
        let control = Control::new(ui, &spec)?;
        let checked = Rc::new(Cell::new(false));
        let hover = Rc::new(Cell::new(false));
        let pressed = Rc::new(Cell::new(false));
        let focus = Rc::new(Cell::new(false));
        {
            let (checked, hover, pressed, focus) = (
                checked.clone(),
                hover.clone(),
                pressed.clone(),
                focus.clone(),
            );
            let theme = ui.theme_handle();
            control.set_painter(Rc::new(move |canvas| {
                let t = theme.get();
                let b = canvas.bounds();
                let dpi = canvas.dpi();
                let frame = b.shrink(((2 * dpi) / 96) as i32);
                let radius = 4.0 * dpi as f32 / 96.0;
                let on = checked.get();
                if on {
                    canvas.fill_rounded_rect(frame, radius, t.accent);
                } else if pressed.get() {
                    canvas.fill_rounded_rect(frame, radius, t.pressed);
                } else if hover.get() {
                    canvas.fill_rounded_rect(frame, radius, t.hover);
                }
                if focus.get() {
                    canvas.stroke_rounded_rect(frame, radius, t.border_focused, 1.0);
                }
                let side = (ICON as u32 * dpi / 96) as i32;
                let icon = Rect::new(
                    (b.left + b.right - side) / 2,
                    (b.top + b.bottom - side) / 2,
                    (b.left + b.right + side) / 2,
                    (b.top + b.bottom + side) / 2,
                );
                let color = if on { t.text_on_accent } else { t.text };
                draw(canvas, glyph, icon, color, dpi);
            }));
        }
        {
            let ui = ui.clone();
            let id = control.id();
            control.on_events(move |event| {
                let redraw = || ui.invalidate(id);
                match event {
                    Event::MouseMove { .. } => {
                        hover.set(true);
                        redraw();
                        None
                    }
                    Event::MouseLeave => {
                        hover.set(false);
                        pressed.set(false);
                        redraw();
                        None
                    }
                    Event::MouseDown {
                        button: MouseButton::Left,
                        ..
                    } => {
                        pressed.set(true);
                        redraw();
                        None
                    }
                    Event::MouseUp {
                        button: MouseButton::Left,
                        ..
                    } => {
                        let click = pressed.replace(false) && hover.get();
                        redraw();
                        click.then(|| msg.clone())
                    }
                    Event::SetFocus | Event::KillFocus => {
                        focus.set(matches!(event, Event::SetFocus));
                        redraw();
                        None
                    }
                    Event::KeyDown { key, .. } if *key == Key::SPACE || *key == Key::RETURN => {
                        Some(msg.clone())
                    }
                    _ => None,
                }
            });
        }
        let _tip = Tooltip::attach(ui, control.id(), tip)?;
        Ok(IconButton {
            control,
            checked,
            _tip,
        })
    }

    /// Shows the button as the selected tool (or not).
    pub fn set_checked(&self, on: bool) {
        if self.checked.replace(on) != on {
            self.control.invalidate();
        }
    }
}

impl<M: 'static> Placeable<M> for IconButton<M> {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, ui: &Ui<M>, _constraints: Constraints) -> Size {
        let side = (SIDE as u32 * ui.dpi() / 96) as i32;
        Size::new(side, side)
    }
}
