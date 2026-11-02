//! A new generation of the material table remakes the panels' lists, on the real
//! widgets, and the old widgets go away.

use std::cell::Cell;

use mscent_render::{Material, MaterialId, MaterialTable};
use xui::xui_canvas::snapshot::{Snapshot, render_with};
use xui::{Dip, Theme};

use super::*;
use crate::model::Selection;

/// The stock materials, a new one at 5, a retired id at 6 and another at 7.
fn extended(stock: &MaterialTable) -> MaterialTable {
    let brick = stock.get(MaterialId(0)).expect("brick");
    let mut table = stock.clone();
    let new = |name: &str| Material {
        name: name.into(),
        gloss: 1,
        texture: brick.texture.clone(),
    };
    table.add(new("concrete"));
    table.add_retired();
    table.add(new("glass"));
    table
}

fn icon_buttons(report: &str) -> usize {
    report.matches("IconButton").count()
}

/// The material list's text with the selected brush on `material`.
fn shown(app: &mut Editor, material: u16) -> String {
    app.state
        .borrow_mut()
        .edit_selected_brush(|b| b.set_material(material));
    app.refresh();
    app.panel.material_text()
}

/// The script runs while the app is built, which is when a test can still reach
/// the handles that a new generation replaces.
fn script(ui: &mut Ui<Msg>) -> xui::backend::Result<Editor> {
    let mut app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
    let first = app.state.borrow().doc.brushes()[0].id;
    app.state.borrow_mut().selection = Some(Selection::Brush(first));
    app.refresh();
    assert_eq!(shown(&mut app, 3), "metal");
    assert_eq!(shown(&mut app, 7), "(other id)", "not in this generation");
    assert_eq!(app.state.borrow().texture_sizes.len(), 5);
    let before = icon_buttons(&ui.layout_report());

    // What is being typed in a field stays through the replacement.
    let min_x = app.panel.min_x();
    let model = app
        .state
        .borrow()
        .selected_brush()
        .expect("a brush")
        .bounds()
        .0[0];
    min_x.set_value(model + 77.0);
    let store = app.studio.materials().clone();
    let stock = store.current().1.expect("stock");
    assert_eq!(store.replace(extended(&stock)), 1);
    app.follow_materials();
    assert_eq!(min_x.value(), model + 77.0, "the typing is gone");
    assert_eq!(app.panel.materials_shown(), 1);
    assert_eq!(app.panel.material_text(), "glass", "the list did follow");

    assert_eq!(shown(&mut app, 3), "metal", "stock ids keep their rows");
    assert_eq!(shown(&mut app, 5), "concrete");
    assert_eq!(shown(&mut app, 6), "(other id)", "a retired id has no name");
    assert_eq!(shown(&mut app, 7), "glass");
    assert_eq!(
        app.state.borrow().texture_sizes.len(),
        8,
        "the sizes followed"
    );
    let report = ui.layout_report();
    assert_eq!(
        icon_buttons(&report),
        before + 2,
        "one more thumbnail per new material, none for the retired id"
    );
    let warnings: Vec<&str> = report
        .lines()
        .filter(|l| l.trim_start().starts_with("! "))
        .collect();
    assert!(warnings.is_empty(), "{warnings:#?}");
    Ok(app)
}

#[test]
fn a_new_generation_remakes_the_lists_and_the_sizes_and_leaves_the_fields_alone() {
    let ran = std::rc::Rc::new(Cell::new(false));
    let flag = ran.clone();
    render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let app = script(ui)?;
            flag.set(true);
            Ok(app)
        },
        |_| {},
    )
    .expect("render");
    assert!(ran.get(), "the script ran to its end");
}
