//! The orthographic view widget: one custom-painted xui node. Middle-drag
//! pans, the wheel zooms, the left button drives the active tool.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::app::Ui;
use xui::backend::{Event, NodeKind, NodeSpec, Result, WidgetId};
use xui::geometry::{Rect, Size};
use xui::layout::Constraints;
use xui::widget::{Control, Placeable};
use xui::{Key, MouseButton};

use super::paint2d::{Scene, paint};
use crate::model::outline::OutlineCache;
use crate::model::{Camera2D, EditorState, Pointer, View};

/// What a view tells the app.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewEvent {
    /// The model changed; refresh panels and repaint the other views.
    Changed,
    /// The pointer is at this world position of the view.
    Hover(View, [f64; 2]),
}

#[derive(Clone, Copy)]
enum Mode {
    Idle,
    Tool,
    Pan(i32, i32),
}

/// A 2D view of the shared editor state.
pub struct Viewport<M: 'static> {
    control: Control<M>,
    auto_fit: Rc<Cell<bool>>,
}

impl<M: 'static> Viewport<M> {
    pub fn new(
        ui: &Ui<M>,
        view: View,
        state: Rc<RefCell<EditorState>>,
        emit: Rc<dyn Fn(ViewEvent) -> M>,
    ) -> Result<Viewport<M>> {
        // Contained: it draws only inside its bounds, so the 3D pane's frames
        // next to it do not repaint it.
        let spec = NodeSpec::new(NodeKind::Custom, Rect::new(0, 0, 10, 10))
            .tab_stop()
            .contained();
        let control = Control::new(ui, &spec)?;
        let cam = Rc::new(RefCell::new(Camera2D::default()));
        let auto_fit = Rc::new(Cell::new(true));
        let outlines = RefCell::new(OutlineCache::default());
        {
            let (state, cam, auto_fit) = (state.clone(), cam.clone(), auto_fit.clone());
            let theme = ui.theme_handle();
            control.set_painter(Rc::new(move |canvas| {
                let b = canvas.bounds();
                let size = (b.width() as f64, b.height() as f64);
                let st = state.borrow();
                if auto_fit.get() {
                    fit_camera(&mut cam.borrow_mut(), &st, view, size);
                }
                let scene = Scene {
                    state: &st,
                    view,
                    cam: *cam.borrow(),
                    theme: theme.get(),
                    outlines: &outlines,
                };
                super::meter::time(super::meter::Lane::View, || paint(canvas, &scene));
            }));
        }
        {
            let ui = ui.clone();
            let id = control.id();
            let (cam, auto_fit) = (cam.clone(), auto_fit.clone());
            let mode = Cell::new(Mode::Idle);
            control.on_events(move |event| {
                let size = ui.bounds(id).size();
                let size = (size.width as f64, size.height as f64);
                let ctx = Ctx {
                    ui: &ui,
                    id,
                    view,
                    state: &state,
                    cam: &cam,
                    size,
                    emit: &emit,
                    auto_fit: &auto_fit,
                };
                handle(&ctx, &mode, event)
            });
        }
        Ok(Viewport { control, auto_fit })
    }

    pub fn invalidate(&self) {
        self.control.invalidate();
    }

    /// Frames the whole document on every paint until the user pans or zooms.
    pub fn refit(&self) {
        self.auto_fit.set(true);
        self.control.invalidate();
    }
}

pub(super) fn fit_camera(cam: &mut Camera2D, st: &EditorState, view: View, size: (f64, f64)) {
    let (a, b) = view.axes();
    let (lo, hi) = st
        .doc
        .bounds()
        .unwrap_or(([-10.0, -10.0, -10.0], [10.0, 10.0, 10.0]));
    cam.fit([lo[a], lo[b]], [hi[a], hi[b]], size);
}

struct Ctx<'a, M: 'static> {
    ui: &'a Ui<M>,
    id: WidgetId,
    view: View,
    state: &'a Rc<RefCell<EditorState>>,
    cam: &'a Rc<RefCell<Camera2D>>,
    size: (f64, f64),
    emit: &'a Rc<dyn Fn(ViewEvent) -> M>,
    auto_fit: &'a Rc<Cell<bool>>,
}

impl<M: 'static> Ctx<'_, M> {
    fn pointer(&self, x: i32, y: i32) -> Pointer {
        let cam = self.cam.borrow();
        Pointer {
            view: self.view,
            pos: cam.to_world(x as f64, y as f64, self.size),
            px: 1.0 / cam.scale,
        }
    }

    fn changed(&self) -> Option<M> {
        self.ui.invalidate(self.id);
        Some((self.emit)(ViewEvent::Changed))
    }
}

fn handle<M: 'static>(c: &Ctx<M>, mode: &Cell<Mode>, event: &Event) -> Option<M> {
    match event {
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Left,
            ..
        } => {
            c.ui.focus(c.id);
            c.ui.set_capture(c.id);
            c.auto_fit.set(false);
            mode.set(Mode::Tool);
            c.state.borrow_mut().pointer_down(c.pointer(*x, *y));
            c.changed()
        }
        Event::MouseDown {
            x,
            y,
            button: MouseButton::Middle,
            ..
        } => {
            c.ui.set_capture(c.id);
            mode.set(Mode::Pan(*x, *y));
            None
        }
        Event::MouseMove { x, y, .. } => match mode.get() {
            Mode::Pan(lx, ly) => {
                c.auto_fit.set(false);
                c.cam
                    .borrow_mut()
                    .pan_pixels((*x - lx) as f64, (*y - ly) as f64);
                mode.set(Mode::Pan(*x, *y));
                c.ui.invalidate(c.id);
                None
            }
            Mode::Tool => {
                c.state.borrow_mut().pointer_move(c.pointer(*x, *y));
                c.changed()
            }
            Mode::Idle => {
                let p = c.pointer(*x, *y);
                Some((c.emit)(ViewEvent::Hover(c.view, p.pos)))
            }
        },
        Event::MouseUp { x, y, button, .. }
            if matches!(button, MouseButton::Left | MouseButton::Middle) =>
        {
            let was = mode.replace(Mode::Idle);
            c.ui.release_capture();
            if matches!(was, Mode::Tool) && *button == MouseButton::Left {
                c.state.borrow_mut().pointer_up(c.pointer(*x, *y));
                return c.changed();
            }
            None
        }
        Event::MouseWheel {
            delta,
            horizontal: false,
            x,
            y,
            ..
        } => {
            let factor = 1.2f64.powi((*delta as i32).signum());
            c.auto_fit.set(false);
            c.cam
                .borrow_mut()
                .zoom_at(*x as f64, *y as f64, c.size, factor);
            c.ui.invalidate(c.id);
            None
        }
        Event::KeyDown { key, .. } => {
            let mut st = c.state.borrow_mut();
            // Delete, Ctrl+Z and Ctrl+Y are window-wide shortcuts (`app.rs`).
            let done = if *key == Key::ESCAPE {
                st.cancel_drag();
                true
            } else {
                false
            };
            drop(st);
            done.then(|| c.changed()).flatten()
        }
        _ => None,
    }
}

impl<M: 'static> Placeable<M> for Viewport<M> {
    fn id(&self) -> WidgetId {
        self.control.id()
    }

    fn measure(&self, _ui: &Ui<M>, _constraints: Constraints) -> Size {
        Size::new(0, 0)
    }
}
