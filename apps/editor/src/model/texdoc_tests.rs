use mscent_texgen::recipe;

use super::*;

fn spec(name: &str) -> &'static OpSpec {
    ops::find(name).expect("in the table")
}

/// A flat fed to the albedo output, which compiles.
fn doc() -> TexDoc {
    let recipe = recipe::read(
        b"miniscent-recipe 1\nsize 16 16\nop 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\n",
    )
    .expect("reads");
    TexDoc::open("a", recipe, 40)
}

fn float_param(d: &TexDoc, block: usize) -> Option<(usize, f32, f32)> {
    let params = d.recipe().blocks.get(block)?.spec.params;
    params.iter().enumerate().find_map(|(i, p)| match p.kind {
        ops::ParamKind::Float { min, max, .. } => Some((i, min, max)),
        _ => None,
    })
}

#[test]
fn the_gloss_is_the_manifests_and_a_fresh_recipe_is_not_dirty() {
    let d = doc();
    assert_eq!(d.recipe().gloss, 40);
    assert!(!d.dirty() && !d.can_undo() && d.page().is_ok());
}

#[test]
fn a_starter_compiles_is_dirty_and_uses_the_table_only() {
    let size = Size::new(32, 32).expect("size");
    let d = TexDoc::starter("new", size, 40);
    assert!(d.page().is_ok(), "{:?}", d.page());
    assert!(d.dirty(), "there is nothing on disk yet");
    assert_eq!(d.recipe().size, size);
    assert_eq!(d.selected(), Some(0));
}

#[test]
fn a_first_page_for_png_files_has_nothing_to_save_until_it_is_edited() {
    let mut d = TexDoc::first_page("brick", Size::new(32, 32).expect("size"), 25);
    assert!(!d.dirty());
    assert_eq!(d.recipe().gloss, 25);
    d.set_gloss(26, false);
    assert!(d.dirty());
}

#[test]
fn every_edit_is_one_undo_step_and_undo_restores_the_page_exactly() {
    let mut d = doc();
    let original = d.recipe().clone();
    let added = d.add(spec("noise"), 1, 0).expect("adds");
    assert_eq!(d.selected(), Some(added));
    d.move_block(added, 2, 0).expect("moves");
    d.resize(added, 2).expect("resizes");
    let copy = d.duplicate(added).expect("duplicates");
    assert_eq!(d.recipe().blocks[copy].y, 1, "below what it copies");
    d.delete(added).expect("deletes");
    assert_eq!(d.recipe().blocks.len(), 3);
    for _ in 0..5 {
        assert!(d.undo());
    }
    assert_eq!(*d.recipe(), original);
    assert!(!d.undo(), "nothing left");
    for _ in 0..5 {
        assert!(d.redo());
    }
    assert_eq!(d.recipe().blocks.len(), 3);
    assert!(!d.redo());
}

#[test]
fn a_new_edit_forgets_what_could_be_redone() {
    let mut d = doc();
    d.move_block(1, 1, 1).expect("moves");
    d.undo();
    assert!(d.can_redo());
    d.resize(1, 2).expect("resizes");
    assert!(!d.can_redo());
}

#[test]
fn an_edit_that_changes_nothing_takes_no_undo_step() {
    let mut d = doc();
    d.move_block(0, 0, 0).expect("same place");
    d.resize(0, 1).expect("same width");
    d.set_gloss(40, false);
    assert!(!d.can_undo());
    assert_eq!(d.revision(), 0);
}

#[test]
fn an_uncompilable_page_is_state_with_the_broken_blocks_named() {
    let mut d = doc();
    assert!(d.page().is_ok());
    d.move_block(1, 5, 5).expect("moves off its input");
    let error = d.page().clone().expect_err("no input above it");
    assert!(error.message.contains("out-albedo"), "{}", error.message);
    assert_eq!(error.blocks, [1]);
    d.undo();
    assert!(d.page().is_ok());
}

#[test]
fn landing_on_another_block_is_allowed_and_marks_both() {
    let mut d = doc();
    d.move_block(1, 0, 0).expect("moves");
    let error = d.page().clone().expect_err("overlap");
    assert_eq!(error.blocks, [0, 1]);
}

#[test]
fn adding_on_a_taken_cell_goes_below_it() {
    let mut d = doc();
    let added = d.add(spec("noise"), 0, 0).expect("adds");
    assert_eq!(
        (d.recipe().blocks[added].x, d.recipe().blocks[added].y),
        (0, 2)
    );
}

#[test]
fn a_block_that_needs_two_inputs_starts_two_columns_wide() {
    let mut d = doc();
    let merge = d.add(spec("merge"), 4, 4).expect("adds");
    assert_eq!(d.recipe().blocks[merge].w, 2);
}

#[test]
fn a_drag_is_one_undo_step_and_its_value_is_the_last() {
    let mut d = doc();
    let block = d.add(spec("noise"), 1, 0).expect("adds");
    let (index, min, max) = float_param(&d, block).expect("noise has a float");
    let before = d.recipe().clone();
    let middle = (min + max) / 2.0;
    for t in [0.1, 0.2, 0.3] {
        let value = Value::Float(min + (max - min) * t);
        d.set_param(block, index, value, true).expect("in range");
    }
    d.set_param(block, index, Value::Float(middle), false)
        .expect("in range");
    assert_eq!(
        d.recipe().blocks[block].params.values()[index],
        Value::Float(middle)
    );
    assert!(d.undo());
    assert_eq!(*d.recipe(), before, "the whole drag went at once");
}

#[test]
fn a_drag_leaves_the_panel_alone_and_undo_does_not() {
    let mut d = doc();
    let block = d.add(spec("noise"), 1, 0).expect("adds");
    let (index, min, _) = float_param(&d, block).expect("noise has a float");
    let epoch = d.panel_epoch();
    d.set_param(block, index, Value::Float(min), true)
        .expect("in range");
    assert_eq!(d.panel_epoch(), epoch, "the slider being held stays");
    d.undo();
    assert_ne!(d.panel_epoch(), epoch);
}

#[test]
fn values_the_table_refuses_are_errors_and_change_nothing() {
    let mut d = doc();
    let block = d.add(spec("noise"), 1, 0).expect("adds");
    let (index, _, max) = float_param(&d, block).expect("noise has a float");
    let before = d.recipe().clone();
    let revision = d.revision();
    for value in [
        Value::Float(f32::NAN),
        Value::Float(f32::INFINITY),
        Value::Float(max + 1.0),
        Value::Text("not a float".to_string()),
    ] {
        assert!(d.set_param(block, index, value, false).is_err());
    }
    assert!(d.set_param(block, 99, Value::Int(0), false).is_err());
    assert!(d.set_param(99, 0, Value::Int(0), false).is_err());
    assert_eq!(*d.recipe(), before);
    assert_eq!(d.revision(), revision);
}

#[test]
fn indices_past_the_page_are_errors() {
    let mut d = doc();
    assert!(d.move_block(9, 0, 0).is_err());
    assert!(d.resize(9, 1).is_err());
    assert!(d.delete(9).is_err());
    assert!(d.duplicate(9).is_err());
    d.select(Some(9));
    assert_eq!(d.selected(), None);
}

#[test]
fn deleting_moves_the_selection_with_the_block_it_was_on() {
    let mut d = doc();
    d.select(Some(1));
    d.delete(0).expect("deletes");
    assert_eq!(d.selected(), Some(0), "still the same block");
    d.delete(0).expect("deletes");
    assert_eq!(d.selected(), None);
}

#[test]
fn the_page_has_a_cap_and_the_history_has_a_bound() {
    let mut d = doc();
    for i in 0..MAX_BLOCKS - 2 {
        let x = (i % 60) as u16;
        d.add(spec("flat"), x, 100).expect("room");
    }
    assert!(d.add(spec("flat"), 0, 200).is_err(), "full");
    assert!(d.duplicate(0).is_err(), "full");
    for _ in 0..HISTORY + 8 {
        d.select(None);
        d.set_gloss(d.recipe().gloss.wrapping_add(1), false);
    }
    assert_eq!(d.past.len(), HISTORY);
}

#[test]
fn hostile_pages_open_and_edit_without_panicking() {
    let pages = [
        // Off the page in every direction, and overflowing a u16.
        "op 65535 65535 65535 flat\nop 65534 0 3 out-albedo\n",
        "op 0 0 0 flat\n",
        // A cycle through store and load.
        "op 0 0 1 load\nop 0 1 1 store\n",
        "op 0 0 1 flat\nop 0 1 1 store\nop 1 0 1 flat\nop 1 1 1 store\n",
    ];
    for page in pages {
        let text = format!("miniscent-recipe 1\nsize 16 16\n{page}");
        let recipe = recipe::read(text.as_bytes()).expect("reads");
        let mut d = TexDoc::open("h", recipe, 0);
        let error = d.page().clone().expect_err(page);
        assert!(!error.blocks.is_empty(), "{page}");
        d.add(spec("flat"), u16::MAX, u16::MAX).expect("adds");
        d.resize(0, u16::MAX).expect("resizes");
        d.move_block(0, u16::MAX, u16::MAX).expect("moves");
        assert!(d.page().is_err());
        while d.undo() {}
    }
}

#[test]
fn clamping_keeps_a_block_on_the_page() {
    assert_eq!(clamp_to_page(-5, -5, 2), (0, 0));
    assert_eq!(clamp_to_page(1000, 1000, 2), (62, 255));
    assert_eq!(clamp_to_page(1000, 0, u16::MAX), (0, 0));
    assert_eq!(clamp_to_page(i32::MAX, i32::MIN, 1), (63, 0));
}
