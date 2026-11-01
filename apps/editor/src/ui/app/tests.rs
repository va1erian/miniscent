//! Drives the real widgets headlessly: injected mouse events and menu
//! messages, then reads pixels from the rendered window.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use xui::backend::Event;
use xui::message::{Modifiers, MouseButton};
use xui::xui_canvas::snapshot::{Snapshot, render_with};
use xui::{Dip, Image, Theme, widget::NumberField};

use super::super::viewport::{Viewport, fit_camera};
use super::*;
use crate::model::Selection;
use crate::model::View;
use xui::widget::Placeable;

/// What the step closure needs from the built app.
struct Probe {
    state: Rc<RefCell<EditorState>>,
    top: Rc<Viewport<Msg>>,
    menu: Rc<Menu<Msg>>,
    min_x: Rc<NumberField<Msg>>,
}

fn save(image: &Image, name: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots/editor");
    std::fs::create_dir_all(&dir).expect("shots dir");
    image.save_png(dir.join(name)).expect("png");
}

fn mouse(stage: &xui::xui_canvas::snapshot::Stage<'_, Msg>, kind: u8, x: i32, y: i32) {
    let modifiers = Modifiers::NONE;
    let button = MouseButton::Left;
    stage.inject(match kind {
        0 => Event::MouseDown {
            x,
            y,
            button,
            modifiers,
        },
        1 => Event::MouseMove { x, y, modifiers },
        _ => Event::MouseUp {
            x,
            y,
            button,
            modifiers,
        },
    });
}

/// Renders the editor with the showcase loaded and the scripted session run.
fn render_session(theme: Theme, scripted: bool) -> (Image, Rc<Cell<u32>>) {
    let probe: Rc<RefCell<Option<Probe>>> = Rc::new(RefCell::new(None));
    let checks = Rc::new(Cell::new(0u32));
    let (p2, c2) = (probe.clone(), checks.clone());
    let image = render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(theme),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *p2.borrow_mut() = Some(Probe {
                state: app.probe().state(),
                menu: app.menu.get(),
                top: app.quad.viewports[0].get(),
                min_x: app.panel.min_x(),
            });
            Ok(app)
        },
        move |stage| {
            if !scripted {
                return;
            }
            let probe = probe.borrow();
            let probe = probe.as_ref().expect("built");
            // Paint once so the views fit the level.
            let _ = stage.ui().capture();
            // Bounds are relative to the enclosing splitter; the top view is the
            // first pane of every one, so it sits at the work area's corner.
            let menu = stage.ui().bounds(Placeable::id(&*probe.menu));
            let rect = stage
                .ui()
                .bounds(Placeable::id(&*probe.top))
                .offset(PALETTE_WIDTH, menu.bottom);
            let size = (rect.width() as f64, rect.height() as f64);
            let mut cam = crate::model::Camera2D::default();
            fit_camera(&mut cam, &probe.state.borrow(), View::Top, size);
            let at = |x: f64, y: f64| {
                let (sx, sy) = cam.to_screen([x, y], size);
                (rect.left + sx as i32, rect.top + sy as i32)
            };
            let n = |s: &Rc<RefCell<EditorState>>| s.borrow().doc.brushes().len();
            let before = n(&probe.state);

            // Box tool: drag a new brush out of empty space east of the level.
            stage.emit(Msg::Tool(1));
            let (x0, y0) = at(30.0, 55.0);
            let (x1, y1) = at(34.0, 58.0);
            mouse(stage, 0, x0, y0);
            mouse(stage, 1, (x0 + x1) / 2, (y0 + y1) / 2);
            mouse(stage, 2, x1, y1);
            assert_eq!(n(&probe.state), before + 1, "drag created a brush");
            let id = match probe.state.borrow().selection {
                Some(Selection::Brush(id)) => id,
                other => panic!("new brush should be selected, got {other:?}"),
            };
            let bounds =
                |s: &Rc<RefCell<EditorState>>| s.borrow().doc.brush(id).expect("alive").bounds();
            assert_eq!(bounds(&probe.state).0[..2], [30.0, 55.0]);
            assert_eq!(probe.min_x.value(), 30.0, "panel shows the selection");

            // Undo and redo through the menu.
            stage.emit(Msg::Menu(cmd::UNDO));
            assert_eq!(n(&probe.state), before);
            stage.emit(Msg::Menu(cmd::REDO));
            assert_eq!(n(&probe.state), before + 1);

            // Select tool: move the brush 4 units west by dragging its middle.
            stage.emit(Msg::Tool(0));
            let (mx, my) = at(32.0, 56.0);
            let (tx, ty) = at(28.0, 56.0);
            mouse(stage, 0, mx, my);
            mouse(stage, 1, tx, ty);
            mouse(stage, 2, tx, ty);
            assert_eq!(bounds(&probe.state).0[0], 26.0);

            // Resize with the right-middle handle (the box is small, so the corner
            // handles overlap it on the narrow top view).
            let (hx, hy) = at(30.0, 56.5);
            let (ex, ey) = at(33.0, 56.5);
            mouse(stage, 0, hx, hy);
            mouse(stage, 1, ex, ey);
            mouse(stage, 2, ex, ey);
            assert_eq!(bounds(&probe.state).1[..2], [33.0, 58.0]);
            assert_eq!(probe.min_x.value(), 26.0);

            // Flip through the menu, then place a light with its tool.
            stage.emit(Msg::Menu(cmd::FLIP));
            assert_eq!(probe.state.borrow().doc.brush(id).unwrap().op, Op::Add);
            stage.emit(Msg::Tool(3));
            let (lx, ly) = at(50.0, 30.0);
            mouse(stage, 0, lx, ly);
            mouse(stage, 2, lx, ly);
            assert!(matches!(
                probe.state.borrow().selection,
                Some(Selection::Entity(_))
            ));
            stage.emit(Msg::Tool(0));
            // Re-select the brush for the panel in the picture.
            let (cx, cy) = at(29.0, 57.0);
            mouse(stage, 0, cx, cy);
            mouse(stage, 2, cx, cy);
            assert_eq!(probe.state.borrow().selection, Some(Selection::Brush(id)));
            checks.set(checks.get() + 1);
        },
    )
    .expect("render");
    (image, c2)
}

fn differs(a: [u8; 4], b: [u8; 4]) -> bool {
    a.iter().zip(&b).any(|(x, y)| x.abs_diff(*y) > 12)
}

#[test]
fn scripted_session_edits_the_model_and_the_pixels() {
    let (image, ran) = render_session(Theme::light(), true);
    assert_eq!(ran.get(), 1, "the scripted step ran to the end");
    save(&image, "scripted-light.png");
    // The top view is the upper-left quadrant: it holds brush fills, so it is
    // not one flat colour.
    let mut colours = std::collections::HashSet::new();
    for y in (40..390).step_by(7) {
        for x in (160..580).step_by(7) {
            colours.insert(image.pixel(x, y).unwrap());
        }
    }
    assert!(colours.len() > 8, "top view has content: {}", colours.len());
    // The empty 3D pane is a flat placeholder, different from the views.
    assert!(differs(
        image.pixel(700, 650).unwrap(),
        image.pixel(300, 100).unwrap()
    ));
}

#[test]
fn dark_theme_renders_a_dark_viewport() {
    let (image, _) = render_session(Theme::dark(), false);
    save(&image, "showcase-dark.png");
    let p = image.pixel(300, 450).unwrap();
    assert!(
        p[0] < 90 && p[1] < 90 && p[2] < 90,
        "viewport background is dark: {p:?}"
    );
}

#[test]
fn light_theme_renders_the_showcase() {
    let (image, _) = render_session(Theme::light(), false);
    save(&image, "showcase-light.png");
    let p = image.pixel(300, 360).unwrap();
    assert!(
        p[0] > 200 && p[1] > 200,
        "viewport background is light: {p:?}"
    );
}

#[test]
fn light_selection_fills_the_panel_and_the_3d_pane_shows_the_built_level() {
    let probe: Rc<RefCell<Option<super::super::Probe>>> = Rc::new(RefCell::new(None));
    let p2 = probe.clone();
    let image = render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *p2.borrow_mut() = Some(app.probe());
            Ok(app)
        },
        move |stage| {
            let probe = probe.borrow();
            let probe = probe.as_ref().expect("built");
            let state = probe.state();
            // No command: the picture comes by itself.
            for _ in 0..30_000 {
                stage.emit(Msg::Step);
                if probe.preview() == Preview::Full {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(probe.preview(), Preview::Full);
            let light = {
                let st = state.borrow();
                st.doc
                    .entities()
                    .iter()
                    .find(|e| matches!(e.entity, mscent_world::Entity::Light { .. }))
                    .map(|e| e.id)
                    .expect("showcase has lights")
            };
            state.borrow_mut().selection = Some(Selection::Entity(light));
            stage.emit(Msg::Tool(0));
            for _ in 0..400 {
                stage.emit(Msg::Step);
                std::thread::sleep(std::time::Duration::from_millis(16));
            }
        },
    )
    .expect("render");
    save(&image, "light-selected.png");
    // The 3D pane is the lower-left of the views: a rendered level has many
    // colours, a placeholder has one.
    let mut colours = std::collections::HashSet::new();
    for y in (500..760).step_by(5) {
        for x in (200..900).step_by(5) {
            colours.insert(image.pixel(x, y).unwrap());
        }
    }
    assert!(
        colours.len() > 200,
        "3D pane shows the level: {}",
        colours.len()
    );
}
