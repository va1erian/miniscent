//! A new generation of the material table remakes the panels' lists, on the real
//! widgets, and the old widgets go away.

use std::cell::RefCell;
use std::rc::Rc;

use mscent_render::{Material, MaterialId, MaterialTable};
use xui::widget::HasText;
use xui::xui_canvas::snapshot::{Snapshot, render_with};
use xui::{Dip, Theme};

use super::*;
use crate::materials::SharedMaterials;
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

/// What the test keeps of the app, which the snapshot owns.
struct Held {
    store: SharedMaterials,
    material: Handle<ComboBox<Msg>>,
}

#[test]
fn a_new_generation_remakes_the_brush_list_the_thumbnails_and_the_sizes() {
    let held: Rc<RefCell<Option<Held>>> = Rc::new(RefCell::new(None));
    let (h2, passed) = (held.clone(), Rc::new(RefCell::new(false)));
    let p2 = passed.clone();
    let probes: Rc<RefCell<Option<super::super::Probe>>> = Rc::new(RefCell::new(None));
    let pr = probes.clone();
    render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *h2.borrow_mut() = Some(Held {
                store: app.studio.materials().clone(),
                material: app.panel.material_handle(),
            });
            *pr.borrow_mut() = Some(app.probe());
            Ok(app)
        },
        move |stage| {
            let held = held.borrow();
            let held = held.as_ref().expect("built");
            let probe = probes.borrow();
            let state = probe.as_ref().expect("built").state();
            let first = state.borrow().doc.brushes()[0].id;
            let shown = |material: u16| {
                state
                    .borrow_mut()
                    .edit_selected_brush(|b| b.set_material(material));
                stage.emit(Msg::View(ViewEvent::Changed));
                held.material.get().text()
            };
            state.borrow_mut().selection = Some(Selection::Brush(first));
            stage.emit(Msg::Step);
            assert_eq!(shown(3), "metal");
            assert_eq!(shown(7), "(other id)", "not in this generation");
            assert_eq!(state.borrow().texture_sizes.len(), 5);
            let before = icon_buttons(&stage.ui().layout_report());
            let old_box = held.material.get();

            let stock = held.store.current().1.expect("stock");
            assert_eq!(held.store.replace(extended(&stock)), 1);
            stage.emit(Msg::Step);

            assert!(
                !Rc::ptr_eq(&old_box, &held.material.get()),
                "the list was remade"
            );
            assert_eq!(shown(3), "metal", "stock ids keep their rows");
            assert_eq!(shown(5), "concrete");
            assert_eq!(shown(6), "(other id)", "a retired id has no name");
            assert_eq!(shown(7), "glass");
            assert_eq!(state.borrow().texture_sizes.len(), 8, "the sizes followed");
            let report = stage.ui().layout_report();
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
            *passed.borrow_mut() = true;
        },
    )
    .expect("render");
    assert!(*p2.borrow(), "the scripted part ran to its end");
}
