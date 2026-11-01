//! The window at a laptop size and a wide size, light and dark: pictures to
//! look at, and a check that xui's layout report has no warnings.

use std::cell::RefCell;
use std::rc::Rc;

use xui::xui_canvas::snapshot::{Snapshot, render_with};
use xui::{Dip, Theme};

use super::*;
use crate::model::Selection;

fn shoot(name: &str, theme: Theme, (w, h): (f32, f32), tool: usize) {
    let report: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    let state: Rc<RefCell<Option<Rc<RefCell<EditorState>>>>> = Rc::new(RefCell::new(None));
    let (r2, s2) = (report.clone(), state.clone());
    let image = render_with(
        Snapshot::new(Dip(w), Dip(h)).theme(theme),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *s2.borrow_mut() = Some(app.probe().state());
            Ok(app)
        },
        move |stage| {
            let state = state.borrow().clone().expect("built");
            stage.emit(Msg::Menu(cmd::BUILD));
            for _ in 0..30_000 {
                stage.emit(Msg::Step);
                if state.borrow().message.starts_with("Build finished") {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let id = state.borrow().doc.brushes()[0].id;
            state.borrow_mut().selection = Some(Selection::Brush(id));
            stage.emit(Msg::Tool(tool));
            *r2.borrow_mut() = stage.ui().layout_report();
        },
    )
    .expect("render");
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots/editor-ui");
    std::fs::create_dir_all(&dir).expect("shots dir");
    image
        .save_png(dir.join(format!("{name}.png")))
        .expect("png");
    let report = report.borrow();
    let warnings: Vec<&str> = report
        .lines()
        .filter(|l| l.trim_start().starts_with("! "))
        .collect();
    assert!(warnings.is_empty(), "{name}: {warnings:#?}");
}

#[test]
fn laptop_and_wide_windows_lay_out_without_warnings() {
    for (suffix, tool) in [("", 0), ("-face", 6)] {
        for (size_name, size) in [("1280", (1280.0, 800.0)), ("2000", (2000.0, 1124.0))] {
            for (theme_name, theme) in [("light", Theme::light()), ("dark", Theme::dark())] {
                let name = format!("{size_name}-{theme_name}{suffix}");
                shoot(&name, theme, size, tool);
            }
        }
    }
}
