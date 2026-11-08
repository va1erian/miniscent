//! The 3D pane (and the full-area Play pane): one custom-painted xui node
//! that shows the newest frame given to [`View3d::set_rgba`] and turns the
//! input it receives into [`mscent_platform::Event`]s, the same vocabulary the
//! game uses. Looking uses xui's pointer lock: the right mouse button locks
//! the pointer while held (Edit), and Play keeps it locked ([`View3d::set_grab`]);
//! the backend's [`Event::PointerDelta`] then drives the look. A backend that
//! cannot lock never sends deltas, so the right-button drag falls back to
//! motion computed from successive positions, and the arrow keys always
//! look. The left button fires in Play.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mscent_platform::{Event as PEvent, Key as PKey, MouseButton as PButton, physical_letter};
use xui::app::Ui;
use xui::backend::{Event, NodeKind, NodeSpec, Result, TextAlign, TextStyle, TextVAlign, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::units::Dip;
use xui::widget::{Control, Placeable};
use xui::{Color, Image, Key, MouseButton};

/// The platform key for a Windows virtual-key code.
pub fn platform_key(k: Key) -> Option<PKey> {
    let c = k.code();
    Some(match c {
        0x1B => PKey::Escape,
        0x0D => PKey::Enter,
        0x20 => PKey::Space,
        0x09 => PKey::Tab,
        0x08 => PKey::Backspace,
        0x2E => PKey::Delete,
        0x10 => PKey::Shift,
        0x11 => PKey::Ctrl,
        0x12 => PKey::Alt,
        0x26 => PKey::Up,
        0x28 => PKey::Down,
        0x25 => PKey::Left,
        0x27 => PKey::Right,
        0x30..=0x39 => PKey::Char(u8::try_from(c).ok()? as char),
        // Movement keys are positions: ZQSD on AZERTY is where WASD is.
        0x41..=0x5A => PKey::Char(physical_letter(u8::try_from(c).ok()? as char)),
        0x70..=0x7B => PKey::F((c - 0x6F) as u8),
        _ => return None,
    })
}

/// Whether `k` moves the fly camera (WASD and R/F by position, Q/E roll).
/// The tool hotkeys must not fire for these while looking around.
pub fn is_fly_key(k: Key) -> bool {
    matches!(
        platform_key(k),
        Some(PKey::Char('W' | 'A' | 'S' | 'D' | 'R' | 'F' | 'Q' | 'E'))
    )
}

/// A mouse press on the picture, for picking: where on the picture it fell
/// (`0..=1` from the top left) and which keys were held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Click {
    pub nx: f32,
    pub ny: f32,
    /// Width over height of the picture shown.
    pub aspect: f32,
    pub right: bool,
    pub ctrl: bool,
    pub alt: bool,
}

/// Where the pointer went with the left button held, on the picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneDrag {
    pub nx: f32,
    pub ny: f32,
    pub aspect: f32,
    /// The button came up here.
    pub released: bool,
}

/// Look state shared between the node's event handler and its owner.
#[derive(Default)]
struct Shared {
    image: RefCell<Option<Image>>,
    events: RefCell<Vec<PEvent>>,
    placeholder: RefCell<String>,
    clicks: RefCell<Vec<Click>>,
    drags: RefCell<Vec<PaneDrag>>,
    /// The left button is held for a drag on the picture.
    left: Cell<bool>,
    /// Wheel notches since the last look.
    wheel: Cell<i32>,
    /// Play wants the pointer locked for as long as the pane is up.
    grab: Cell<bool>,
    /// A `PointerDelta` arrived for this lock, so the backend really locks and
    /// position-difference look must not be added on top.
    delta_seen: Cell<bool>,
    /// The backend ended a grab the pane asked for (Esc or focus loss).
    lock_lost: Cell<bool>,
    /// The right button is held for a mouse look.
    looking: Cell<bool>,
    /// Where input goes the moment it arrives, instead of waiting in `events`.
    forward: RefCell<Option<crate::studio::worker::Input>>,
}

/// A pane that draws a frame and collects input.
pub struct View3d<M: 'static> {
    control: Control<M>,
    shared: Rc<Shared>,
    ui: Ui<M>,
}

impl<M: 'static> View3d<M> {
    pub fn new(ui: &Ui<M>) -> Result<View3d<M>> {
        let control = Control::new(
            ui,
            // Contained: a new frame repaints the pane and nothing around it.
            &NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10))
                .tab_stop()
                .contained(),
        )?;
        let shared = Rc::new(Shared::default());
        *shared.placeholder.borrow_mut() = "3D view".into();
        {
            let shared = shared.clone();
            let theme = ui.theme_handle();
            control.set_painter(Rc::new(move |canvas| {
                let start = std::time::Instant::now();
                let theme = theme.get();
                let b = canvas.bounds();
                canvas.clear(theme.input_background.lerp(Color::rgb(0, 0, 0), 0.35));
                if let Some(img) = shared.image.borrow().as_ref() {
                    canvas.draw_image(img, fit(b, img.width(), img.height()));
                } else {
                    let mut style = TextStyle::new(theme.text_secondary, Dip(12.0));
                    style.align = TextAlign::Center;
                    style.valign = TextVAlign::Middle;
                    canvas.draw_text(shared.placeholder.borrow().as_str(), b, &style);
                }
                canvas.stroke_rect(b, theme.border, 1.0);
                super::meter::note(super::meter::Lane::Pane, start);
            }));
        }
        {
            let ui = ui.clone();
            let id = control.id();
            let shared = shared.clone();
            let look: Cell<Option<(i32, i32)>> = Cell::new(None);
            control.on_events(move |event| {
                handle(&ui, id, &shared, &look, event);
                None
            });
        }
        Ok(View3d {
            control,
            shared,
            ui: ui.clone(),
        })
    }

    /// Keeps the pointer locked (hidden, relative motion) while `on`, as Play
    /// does; off releases it and any look in progress. Safe to repeat.
    pub fn set_grab(&self, on: bool) {
        self.shared.grab.set(on);
        self.shared.lock_lost.set(false);
        self.shared.delta_seen.set(false);
        let id = self.control.id();
        if on {
            self.ui.set_pointer_lock(id, true);
        } else {
            self.ui.set_pointer_lock(id, false);
        }
    }

    /// Whether the backend ended the grab on its own (Esc, focus loss) since
    /// the last call.
    pub fn take_lock_lost(&self) -> bool {
        self.shared.lock_lost.replace(false)
    }

    /// Shows a frame of `w` by `h` RGBA pixels built off the UI thread; the
    /// bytes are moved in, not copied. Ignored if the length is wrong.
    pub fn set_rgba(&self, w: u32, h: u32, rgba: Vec<u8>) {
        if let Ok(img) = Image::from_rgba(w, h, rgba) {
            *self.shared.image.borrow_mut() = Some(img);
            self.control.invalidate();
        }
    }

    /// Replaces the frame with a text prompt.
    pub fn show_text(&self, text: &str) {
        *self.shared.image.borrow_mut() = None;
        *self.shared.placeholder.borrow_mut() = text.to_string();
        self.control.invalidate();
    }

    /// The input that arrived since the last call.
    pub fn take_events(&self) -> Vec<PEvent> {
        std::mem::take(&mut *self.shared.events.borrow_mut())
    }

    /// The presses that fell on the picture since the last call.
    pub fn take_clicks(&self) -> Vec<Click> {
        std::mem::take(&mut *self.shared.clicks.borrow_mut())
    }

    /// The left-button drags on the picture since the last call.
    pub fn take_drags(&self) -> Vec<PaneDrag> {
        std::mem::take(&mut *self.shared.drags.borrow_mut())
    }

    /// Wheel notches since the last call.
    pub fn take_wheel(&self) -> i32 {
        self.shared.wheel.replace(0)
    }

    /// Queues a drag as if it came from the user (scripted runs).
    #[allow(dead_code, reason = "used by the self-test and unit tests")]
    pub fn push_drag(&self, d: PaneDrag) {
        self.shared.drags.borrow_mut().push(d);
    }

    /// Queues a click as if it came from the user (scripted runs).
    #[allow(dead_code, reason = "used by the self-test and unit tests")]
    pub fn push_click(&self, c: Click) {
        self.shared.clicks.borrow_mut().push(c);
    }

    /// A checksum of the pixels shown and their size, `None` before the
    /// first frame (scripted runs compare pictures with it).
    #[allow(dead_code, reason = "used by the self-test and unit tests")]
    pub fn frame_checksum(&self) -> Option<(u32, u32, u64)> {
        let img = self.shared.image.borrow();
        let img = img.as_ref()?;
        let sum = img
            .pixels()
            .iter()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
                (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
            });
        Some((img.width(), img.height(), sum))
    }

    /// The pixels shown (width, height, RGBA bytes), `None` before the first
    /// frame (scripted runs measure how much a picture changed).
    #[allow(dead_code, reason = "used by the self-test")]
    pub fn frame_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        let img = self.shared.image.borrow();
        let img = img.as_ref()?;
        Some((img.width(), img.height(), img.pixels().to_vec()))
    }

    pub fn set_visible(&self, visible: bool) {
        self.control.set_visible(visible);
    }

    /// Whether `key` currently flies the camera rather than picking a tool:
    /// the right button is held to look around and it is a movement letter.
    pub fn flies(&self, key: Key) -> bool {
        self.shared.looking.get() && is_fly_key(key)
    }

    pub fn focus(&self) {
        self.control.focus();
    }

    /// Pixel size of the pane.
    pub fn size(&self) -> (u32, u32) {
        let b = self.control.bounds();
        (b.width().max(0) as u32, b.height().max(0) as u32)
    }

    /// Queues an event as if it came from the user (scripted runs).
    #[allow(dead_code, reason = "used by the self-test and unit tests")]
    pub fn push_event(&self, e: PEvent) {
        self.shared.events.borrow_mut().push(e);
    }
}

fn handle<M: 'static>(
    ui: &Ui<M>,
    id: WidgetId,
    shared: &Shared,
    look: &Cell<Option<(i32, i32)>>,
    event: &Event,
) {
    let push = |e| shared.push(e);
    match *event {
        Event::MouseDown {
            x,
            y,
            button,
            modifiers,
        } => {
            ui.focus(id);
            let picks = |right| {
                let (nx, ny, aspect) = on_picture(ui, id, shared, x, y)?;
                Some(Click {
                    nx,
                    ny,
                    aspect,
                    right,
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                })
            };
            match button {
                MouseButton::Right if modifiers.alt => {
                    if let Some(c) = picks(true) {
                        shared.clicks.borrow_mut().push(c);
                    }
                }
                MouseButton::Right => {
                    ui.set_capture(id);
                    look.set(Some((x, y)));
                    shared.looking.set(true);
                    if !shared.grab.get() {
                        shared.delta_seen.set(false);
                    }
                    ui.set_pointer_lock(id, true);
                }
                MouseButton::Left => {
                    if shared.grab.get() {
                        // Click to take the pointer back if the lock did not start.
                        ui.set_pointer_lock(id, true);
                    }
                    if let Some(c) = picks(false) {
                        shared.clicks.borrow_mut().push(c);
                        if !shared.grab.get() {
                            ui.set_capture(id);
                            shared.left.set(true);
                        }
                    }
                    push(PEvent::MouseButton {
                        button: PButton::Left,
                        pressed: true,
                    });
                }
                _ => {}
            }
        }
        Event::MouseUp { button, x, y, .. } => match button {
            MouseButton::Right => {
                look.set(None);
                shared.looking.set(false);
                ui.release_capture();
                if !shared.grab.get() {
                    ui.set_pointer_lock(id, false);
                }
            }
            MouseButton::Left => {
                if shared.left.replace(false) {
                    ui.release_capture();
                    push_drag(ui, id, shared, x, y, true);
                }
                push(PEvent::MouseButton {
                    button: PButton::Left,
                    pressed: false,
                });
            }
            _ => {}
        },
        Event::MouseWheel { delta, .. } => {
            shared
                .wheel
                .set(shared.wheel.get() + (delta as i32).signum());
        }
        Event::MouseMove { x, y, .. } => {
            if shared.left.get() {
                push_drag(ui, id, shared, x, y, false);
            }
            if let Some((lx, ly)) = look.get() {
                look.set(Some((x, y)));
                if !shared.delta_seen.get() {
                    push(PEvent::MouseMotion {
                        dx: (x - lx) as f32,
                        dy: (y - ly) as f32,
                    });
                }
            }
        }
        Event::PointerDelta { dx, dy } => {
            shared.delta_seen.set(true);
            push(PEvent::MouseMotion {
                dx: dx as f32,
                dy: dy as f32,
            });
        }
        Event::PointerLockLost => {
            shared.looking.set(false);
            if look.take().is_some() {
                ui.release_capture();
            }
            if shared.grab.get() {
                shared.lock_lost.set(true);
            }
        }
        Event::KeyDown { key, repeat, .. } if repeat <= 1 => {
            if let Some(code) = platform_key(key) {
                push(PEvent::Key {
                    code,
                    pressed: true,
                });
            }
        }
        Event::KeyUp { key, .. } => {
            if let Some(code) = platform_key(key) {
                push(PEvent::Key {
                    code,
                    pressed: false,
                });
            }
        }
        Event::KillFocus | Event::CaptureChanged => {
            look.set(None);
            if shared.left.replace(false) {
                shared.drags.borrow_mut().push(PaneDrag {
                    nx: -1.0,
                    ny: -1.0,
                    aspect: 1.0,
                    released: true,
                });
            }
            shared.looking.set(false);
            if !shared.grab.get() {
                ui.set_pointer_lock(id, false);
            }
            push(PEvent::FocusLost);
        }
        _ => {}
    }
}

/// Where `(x, y)` falls on the picture, with the picture's aspect, if it does.
fn on_picture<M: 'static>(
    ui: &Ui<M>,
    id: WidgetId,
    shared: &Shared,
    x: i32,
    y: i32,
) -> Option<(f32, f32, f32)> {
    let b = ui.bounds(id);
    let img = shared.image.borrow();
    let img = img.as_ref()?;
    let r = fit(
        Rect::new(0, 0, b.width(), b.height()),
        img.width(),
        img.height(),
    );
    let (w, h) = (r.width().max(1) as f32, r.height().max(1) as f32);
    let (nx, ny) = ((x - r.left) as f32 / w, (y - r.top) as f32 / h);
    ((0.0..=1.0).contains(&nx) && (0.0..=1.0).contains(&ny)).then_some((nx, ny, w / h))
}

fn push_drag<M: 'static>(
    ui: &Ui<M>,
    id: WidgetId,
    shared: &Shared,
    x: i32,
    y: i32,
    released: bool,
) {
    let place = on_picture(ui, id, shared, x, y);
    // Off the picture a stroke waits, but its end must still arrive.
    let (nx, ny, aspect) = match place {
        Some(p) => p,
        None if released => (-1.0, -1.0, 1.0),
        None => return,
    };
    shared.drags.borrow_mut().push(PaneDrag {
        nx,
        ny,
        aspect,
        released,
    });
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

mod play;
#[cfg(test)]
#[path = "view3d_tests.rs"]
mod tests;
