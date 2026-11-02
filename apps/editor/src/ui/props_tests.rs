//! A remount that fails keeps the old widgets working and is tried again. Neither
//! xui nor its headless backend can be told to fail a remount of these lists, so
//! the failures are forced at `show_materials_with`'s mount step; the first test
//! shows that this is what a real failed mount looks like.

use std::cell::Cell;
use std::rc::Rc;

use mscent_render::{Material, MaterialId, MaterialTable};
use xui::backend::BackendError;
use xui::xui_canvas::snapshot::{Snapshot, render_with};
use xui::{Dip, Theme};

use super::*;
use crate::model::{Document, Selection};
use crate::ui::Editor;

/// How many ids the stock manifest fills.
const STOCK: usize = mscent_world::materials::LAST_SKY as usize + 1;
/// The View rows and thumbnails that follow the textures.
const VIEWS: usize = mscent_world::materials::VIEW_SLOTS as usize;

fn stock_plus_one() -> MaterialTable {
    let mut table = miniscent_game::assets::load_materials(None).expect("stock");
    let brick = table.get(MaterialId(0)).expect("brick").texture.clone();
    table.add(Material {
        name: "concrete".into(),
        gloss: 1,
        texture: brick,
    });
    table
}

/// Runs `check` on a real editor with a brush selected, while it is being built.
fn with_editor(check: impl FnOnce(&mut Editor) + 'static) {
    let ran = Rc::new(Cell::new(false));
    let flag = ran.clone();
    render_with(
        Snapshot::new(Dip(1280.0), Dip(800.0)).theme(Theme::light()),
        move |ui| {
            let doc = Document::from_level(&mscent_world::showcase_level());
            let mut app = Editor::build(ui, doc)?;
            let first = app.state.borrow().doc.brushes()[0].id;
            app.state.borrow_mut().selection = Some(Selection::Brush(first));
            check(&mut app);
            flag.set(true);
            Ok(app)
        },
        |_| {},
    )
    .expect("render");
    assert!(ran.get(), "the check ran");
}

#[test]
fn a_real_failed_mount_returns_an_error_and_keeps_the_old_list() {
    with_editor(|app| {
        let old = app.panel.material.get();
        let host = app.panel.material_host.get();
        let failing = column().child(build(|_| -> xui::backend::Result<Label<Msg>> {
            Err(BackendError::CreateFailed("forced"))
        }));
        assert!(host.set_layout(failing).is_err());
        old.select(3);
        assert_eq!(old.selected(), 3, "the old list still works");
    });
}

#[test]
fn a_failed_remount_keeps_the_old_lists_and_the_next_try_succeeds() {
    with_editor(|app| {
        let bigger = stock_plus_one();
        let state = app.state.clone();
        let panel = &mut app.panel;
        let old = panel.material.get();

        // The first mount, the thumbnails', fails.
        let fail = |_: &Host, _: Layout<Msg>| Err(BackendError::CreateFailed("forced"));
        assert!(panel.show_materials_with(1, Some(&bigger), &fail).is_err());
        assert_eq!(panel.materials_shown(), 0, "still to be done");
        assert_eq!(panel.face.thumb_count(), STOCK + VIEWS);
        panel.sync(&mut state.borrow_mut());

        // The thumbnails mount and the second one, the list's, fails.
        let mounts = Cell::new(0);
        let second_fails = |host: &Host, layout: Layout<Msg>| {
            mounts.set(mounts.get() + 1);
            if mounts.get() == 2 {
                return Err(BackendError::CreateFailed("forced"));
            }
            host.set_layout(layout)
        };
        let got = panel.show_materials_with(1, Some(&bigger), &second_fails);
        assert!(got.is_err());
        assert_eq!(panel.materials_shown(), 0, "still to be done");
        assert_eq!(
            panel.face.thumb_count(),
            STOCK + 1 + VIEWS,
            "the thumbnails are consistent"
        );
        assert!(
            Rc::ptr_eq(&old, &panel.material.get()),
            "the old list stays"
        );
        assert_eq!(
            panel.materials.labels().len(),
            STOCK + 1 + VIEWS,
            "with its own rows"
        );
        panel.sync(&mut state.borrow_mut());
        assert!(panel.note_failure(1), "reported once");
        assert!(!panel.note_failure(1), "not on every tick");

        // The next tick's try works.
        let sizes = panel.show_materials(1, Some(&bigger)).expect("mounts");
        assert_eq!(sizes.len(), STOCK + 1);
        assert_eq!(panel.materials_shown(), 1);
        assert!(!Rc::ptr_eq(&old, &panel.material.get()), "a new list");
        assert_eq!(panel.materials.labels().len(), STOCK + 2 + VIEWS);
        panel.sync(&mut state.borrow_mut());
    });
}
