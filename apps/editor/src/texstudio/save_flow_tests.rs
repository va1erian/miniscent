//! Apply, Save and opening against a real directory: the ids the running table and
//! the disk give, what Save asks about, and what the UI thread never does.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use mscent_render::manifest::{self, Entry};
use mscent_render::{MaterialId, MaterialTable};
use mscent_texgen::recipe;
use mscent_world::materials::NAMES;

use super::material::STOCK;
use super::tests::{dir_of, open_and_wait, store_of, studio, wait_bake};
use super::*;

/// Ticks to run while something is held up.
const TICKS: u32 = 200;

#[test]
fn the_stock_list_is_the_shipped_manifests_first_eighteen_rows() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures/materials.txt");
    let entries = manifest::parse(&std::fs::read(path).expect("reads")).expect("parses");
    let names: Vec<&str> = entries
        .iter()
        .take(STOCK.len())
        .map(|e| match e {
            Entry::Material { name, .. } => name.as_str(),
            Entry::Retired => "",
        })
        .collect();
    assert_eq!(names, STOCK);
}

#[test]
fn save_asks_about_every_stock_material_and_nothing_else() {
    let mut s = TexStudio::new(store_of(&STOCK));
    s.set_open(true);
    let d = dir_of("consent", &STOCK);
    s.store.set_dir(d.clone());
    for name in STOCK {
        open_and_wait(&mut s, name);
        assert_eq!(
            s.save(),
            SaveStart::NeedsConsent(name.to_string()),
            "{name}"
        );
        assert!(!s.is_baking(), "nothing happens until the answer");
        s.consent(name);
        assert_eq!(s.save(), SaveStart::Started, "{name}");
        assert!(wait_bake(&mut s).starts_with(&format!("Saved {name}")));
    }
    s.create("mine", 16).expect("creates");
    assert_eq!(s.save(), SaveStart::Started);
    assert!(wait_bake(&mut s).starts_with("Saved mine (id 18)"));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn saving_a_material_applied_after_an_unsaved_one_is_refused_until_that_is_saved() {
    let mut s = studio();
    let d = dir_of("ids", &NAMES);
    s.store.set_dir(d.clone());
    // Alpha takes id 5 in the running table, and is not on disk.
    assert!(s.apply());
    assert_eq!(wait_bake(&mut s), "Applied test");
    s.create("beta", 16).expect("creates");
    assert_eq!(s.save(), SaveStart::Started);
    let said = wait_bake(&mut s);
    assert!(said.starts_with("not saved"), "{said}");
    assert!(said.contains("save test first"), "{said}");
    let rows = std::fs::read_to_string(d.join("materials.txt")).expect("reads");
    assert!(!rows.contains("beta"), "{rows}");
    assert!(!d.join("beta.mst").exists());
    assert!(s.table().expect("table").find("beta").is_none());
    // In the right order the two agree.
    open_and_wait(&mut s, "test");
    assert_eq!(s.save(), SaveStart::Started);
    assert!(wait_bake(&mut s).starts_with("Saved test (id 5)"));
    open_and_wait(&mut s, "beta");
    assert_eq!(s.save(), SaveStart::Started);
    assert!(wait_bake(&mut s).starts_with("Saved beta (id 6)"));
    let on_disk = MaterialTable::load_dir(&d).expect("the game can load it");
    let here = s.table().expect("table");
    for (name, id) in [("test", 5), ("beta", 6)] {
        assert_eq!(on_disk.find(name), Some(MaterialId(id)));
        assert_eq!(here.find(name), Some(MaterialId(id)));
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_material_applied_under_a_new_id_is_unsaved_until_it_is_saved() {
    let mut s = studio();
    let d = dir_of("unsaved", &NAMES);
    s.store.set_dir(d.clone());
    let ids = BTreeSet::from([0, 5]);
    assert!(s.unsaved_among(&ids).is_empty(), "nothing applied yet");
    assert!(s.apply());
    wait_bake(&mut s);
    assert_eq!(
        s.unsaved_among(&ids),
        ["test"],
        "brick is on disk, test is not"
    );
    assert!(s.save() == SaveStart::Started);
    wait_bake(&mut s);
    assert!(s.unsaved_among(&ids).is_empty());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_new_name_is_refused_when_a_recipe_of_that_name_is_already_there() {
    let mut s = studio();
    let d = dir_of("taken", &NAMES);
    std::fs::write(d.join("foo.mst"), "not listed").expect("writes");
    s.store.set_dir(d.clone());
    let why = s.create("foo", 16).expect_err("refused");
    assert!(why.contains("foo.mst"), "{why}");
    assert!(s.create("fresh", 16).is_ok());
    assert_eq!(
        std::fs::read(d.join("foo.mst")).expect("reads"),
        b"not listed"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn opening_a_recipe_is_read_on_the_worker_and_ticks_do_not_wait_for_it() {
    let mut s = TexStudio::new(store_of(&NAMES));
    s.set_open(true);
    let d = dir_of("open", &NAMES);
    s.store.set_dir(d.clone());
    s.tick();
    let locks = s.worker.as_ref().expect("worker").locks();
    {
        // The worker cannot get to the file, so a recipe on show can only have
        // been read here.
        let _held = locks.gate();
        s.open_material("rock");
        assert!(s.is_opening() && s.needs_ticks());
        for _ in 0..TICKS {
            s.tick();
            assert!(s.doc().is_none(), "read on the UI thread");
        }
        s.open_material("brick");
        assert!(
            s.tick()
                .message
                .is_some_and(|m| m.contains("still opening")),
            "one at a time"
        );
    }
    open_and_wait(&mut s, "rock");
    assert_eq!(s.doc().expect("open").name(), "rock");
    assert!(!s.is_opening());
    let _ = std::fs::remove_dir_all(d);
}

/// A page the cancel flag stops at its first block, as the worker's quitting does.
#[test]
fn a_bake_stops_between_blocks_when_told_to() {
    let text = "miniscent-recipe 1\nsize 64 64\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";
    let recipe = recipe::read(text.as_bytes()).expect("reads");
    let graph = mscent_texgen::graph::compile(&recipe).expect("compiles");
    let quit = AtomicBool::new(true);
    let mut cache = mscent_texgen::graph::Cache::new(1 << 20);
    let applied = eval::bake(&recipe, &graph, &mut cache, &quit);
    assert!(applied.is_err_and(|e| e == "cancelled"));
    let saved = eval::bake_saved(&recipe, &quit);
    assert!(saved.is_err_and(|e| e == "cancelled"));
    let go = AtomicBool::new(false);
    assert!(eval::bake_saved(&recipe, &go).is_ok());
}
