#![forbid(unsafe_code)]

//! The GL fallback path exercised without a GPU: the offscreen backend paints a
//! [`GlWidget`]'s software fallback, deterministically and headlessly.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui_core::app::{App, Ui, run_app};
use xui_core::backend::{Backend, NodeKind, NodeSpec, ParentRef, PlatformSpec};
use xui_core::{Canvas, Color, Dip, Rect, Theme};

use crate::gl::GlWidget;
use crate::{OffscreenBackend, RgbaImage};

/// A widget that records its software fallback paint and fills `bounds`.
struct FallbackProbe {
    painted: Rc<Cell<bool>>,
}

impl GlWidget for FallbackProbe {
    fn paint(&self, canvas: &mut dyn Canvas, bounds: Rect, _theme: &Theme) {
        self.painted.set(true);
        canvas.fill_rect(bounds, Color::rgb(255, 0, 0));
    }

    fn paint_gl(&self, _gl: &crate::glow::Context, _bounds: Rect, _theme: &Theme) {}
}

/// An app that holds nothing; the offscreen backend needs an `App` to drive it.
struct Noop;

impl App for Noop {
    type Msg = ();
    fn update(&mut self, _msg: (), _ui: &mut Ui<()>) {}
}

#[test]
fn a_gl_widget_falls_back_to_software_offscreen() {
    let backend = Rc::new(OffscreenBackend::new());
    let backend_for_run: Rc<dyn Backend> = backend.clone();
    let painted = Rc::new(Cell::new(false));
    let painted_for_make = Rc::clone(&painted);
    let captured = Rc::new(RefCell::new(None));
    let captured_for_make = Rc::clone(&captured);

    let _ = run_app(
        backend_for_run,
        PlatformSpec::new("gl fallback").size(Dip(64.0), Dip(64.0)),
        move |ui| {
            backend.set_gl_content(
                ui.window(),
                FallbackProbe {
                    painted: painted_for_make,
                },
            );
            *captured_for_make.borrow_mut() = backend.render(ui.window());
            Noop
        },
    );

    assert!(painted.get(), "the fallback paint did not run");
    let image: RgbaImage = captured.borrow().clone().expect("a rendered window");
    assert_eq!(
        image.pixel(32, 32),
        Some([255, 0, 0, 255]),
        "the fallback filled the client area"
    );
    assert_eq!(
        image.pixel(0, 0),
        Some([255, 0, 0, 255]),
        "the fallback covers the whole client area, not a corner"
    );
}

#[test]
fn a_gl_widget_on_a_node_falls_back_inside_its_pane() {
    let backend = OffscreenBackend::new();
    let spec = PlatformSpec::new("gl pane").size(Dip(64.0), Dip(64.0));
    let window = backend.open_window_at(&spec, 96).expect("a window");
    let node = backend
        .create(
            ParentRef::Window(window),
            &NodeSpec::new(NodeKind::Container, Rect::new(16, 16, 48, 48)),
        )
        .expect("a node");
    let painted = Rc::new(Cell::new(false));
    backend.set_gl_content_on(
        node,
        FallbackProbe {
            painted: Rc::clone(&painted),
        },
    );

    let image = backend.render(window).expect("a rendered window");
    assert!(painted.get(), "the node's fallback paint did not run");
    assert_eq!(
        image.pixel(32, 32),
        Some([255, 0, 0, 255]),
        "the fallback filled the node's pane"
    );
    let background = Theme::light().background;
    assert_eq!(
        image.pixel(4, 4),
        Some([background.r, background.g, background.b, 255]),
        "outside the pane stays the window background"
    );
}
