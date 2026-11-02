//! The workspace's loop without widgets: real worker, real recipes, pixels read
//! back and compared with a bake of the same page.

use std::time::{Duration, Instant};

use mscent_render::{Material, MaterialId, Texture};
use mscent_texgen::bake;
use mscent_texgen::ops::{self, Value};
use mscent_texgen::recipe;
use mscent_world::materials::NAMES;

use super::*;
use crate::materials::MaterialStore;

/// Long enough for any page here on a busy machine; only reached by a failure.
const PATIENCE: Duration = Duration::from_secs(60);

/// A store with the renderer's five stock names, a 16 px texture each, so no test
/// reads assets.
pub(super) fn store() -> SharedMaterials {
    store_of(&NAMES)
}

/// A store with these materials, a 16 px texture each.
pub(super) fn store_of(names: &[&str]) -> SharedMaterials {
    let store = SharedMaterials::new(MaterialStore::default());
    let mut table = MaterialTable::new();
    for name in names {
        table.add(Material {
            name: name.to_string(),
            gloss: 40,
            texture: Texture::new(16, 16, vec![0xff10_2030; 256]).expect("texture"),
        });
    }
    store.replace(table);
    store
}

/// A studio open on a new material, 64 px by default.
pub(super) fn studio() -> TexStudio {
    let mut s = TexStudio::new(store());
    s.set_open(true);
    s.create("test", 64).expect("creates");
    s
}

/// Ticks until the picture has caught up, keeping the last picture and thumbnails.
pub(super) fn settle(s: &mut TexStudio) -> (Option<Picture>, Option<Thumbs>) {
    let end = Instant::now() + PATIENCE;
    let (mut picture, mut thumbs) = (None, None);
    loop {
        let tick = s.tick();
        picture = tick.picture.or(picture);
        thumbs = tick.thumbs.or(thumbs);
        if s.settled() {
            return (picture, thumbs);
        }
        assert!(Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// Opens a material and ticks until its file is read; what the last tick said,
/// which is the reason when it could not be.
pub(super) fn open_and_wait(s: &mut TexStudio, name: &str) -> Option<String> {
    s.open_material(name);
    let end = Instant::now() + PATIENCE;
    let mut said = None;
    while s.is_opening() {
        said = s.tick().message;
        assert!(Instant::now() < end, "never opened");
        std::thread::sleep(Duration::from_millis(1));
    }
    said
}

/// Replaces the page with this one, as a recipe file would.
pub(super) fn set_page(s: &mut TexStudio, side: u32, body: &str) {
    let text = format!("miniscent-recipe 1\nsize {side} {side}\n{body}");
    let recipe = recipe::read(text.as_bytes()).expect("reads");
    s.docs
        .insert("test".to_string(), TexDoc::open("test", recipe, 40));
    s.switch_to("test");
}

fn colour_param(s: &TexStudio, block: usize) -> usize {
    let doc = s.doc().expect("open");
    let params = doc.recipe().blocks[block].spec.params;
    params
        .iter()
        .position(|p| matches!(p.kind, ops::ParamKind::Color { .. }))
        .expect("a colour")
}

fn set_colour(s: &mut TexStudio, block: usize, c: [f32; 3], live: bool) {
    let index = colour_param(s, block);
    let value = Value::Color([c[0], c[1], c[2], 1.0]);
    s.edit(|d| d.set_param(block, index, value, live))
        .expect("edits");
    s.note_drag(live);
}

/// The RGBA a recipe's albedo should show.
fn baked_rgba(s: &TexStudio) -> Vec<u8> {
    let recipe = s.doc().expect("open").recipe().clone();
    let maps = bake(&recipe, recipe.size).expect("bakes");
    let mut out = Vec::new();
    for p in maps.albedo {
        let [_, r, g, b] = p.to_be_bytes();
        out.extend_from_slice(&[r, g, b, 255]);
    }
    out
}

const PAGE: &str = "op 0 0 1 noise seed=3\nop 1 0 1 flat color=0.2,0.5,0.9\nop 0 1 2 merge mode=multiply\nop 0 2 1 out-albedo\nop 2 0 1 noise seed=9\nop 2 1 1 out-height normal_strength=8\n";

#[test]
fn the_preview_shows_the_exact_albedo_of_the_page_at_full_size() {
    let mut s = studio();
    set_page(&mut s, 256, PAGE);
    s.set_tab(Tab::Albedo);
    let (picture, thumbs) = settle(&mut s);
    let picture = picture.expect("a picture");
    assert!(picture.full);
    assert_eq!((picture.width, picture.height), (256, 256));
    assert_eq!(picture.rgba, baked_rgba(&s));
    assert_eq!(thumbs.expect("thumbnails").images.len(), 6);
}

#[test]
fn undo_gives_back_identical_pixels() {
    let mut s = studio();
    set_page(&mut s, 64, PAGE);
    s.set_tab(Tab::Albedo);
    let before = settle(&mut s).0.expect("a picture").rgba;
    set_colour(&mut s, 1, [0.9, 0.1, 0.1], false);
    let changed = settle(&mut s).0.expect("a picture").rgba;
    assert_ne!(changed, before);
    assert!(s.undo());
    assert_eq!(settle(&mut s).0.expect("a picture").rgba, before);
    assert!(s.redo());
    assert_eq!(settle(&mut s).0.expect("a picture").rgba, changed);
}

#[test]
fn a_tab_is_its_map_and_a_missing_map_says_so() {
    let mut s = studio();
    set_page(
        &mut s,
        32,
        "op 0 0 1 flat color=0.5,0.25,1\nop 0 1 1 out-albedo\n",
    );
    for (tab, want) in [
        (Tab::Height, [0, 0, 0, 255]),
        (Tab::Normal, [128, 128, 255, 255]),
        (Tab::Specular, [255, 255, 255, 255]),
    ] {
        s.set_tab(tab);
        let picture = settle(&mut s).0.expect("a picture");
        assert_eq!(picture.rgba, want, "{tab:?}");
        assert!(picture.note.is_some(), "{tab:?} says the page has none");
    }
    s.set_tab(Tab::Block);
    let picture = settle(&mut s).0.expect("a picture");
    assert!(
        picture.rgba.is_empty() && picture.note.is_some(),
        "nothing selected"
    );
    s.edit(|d| {
        d.select(Some(0));
        Ok(())
    });
    let picture = settle(&mut s).0.expect("a picture");
    assert_eq!(&picture.rgba[..4], &[128, 64, 255, 255]);
}

#[test]
fn height_and_specular_read_red_and_normals_come_from_the_height() {
    let mut s = studio();
    let page = "op 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\nop 1 0 1 noise seed=2\nop 1 1 1 out-height normal_strength=6\nop 2 0 1 flat color=0.25,1,1\nop 2 1 1 out-specular\n";
    set_page(&mut s, 64, page);
    let recipe = s.doc().expect("open").recipe().clone();
    let maps = bake(&recipe, recipe.size).expect("bakes");
    s.set_tab(Tab::Specular);
    let picture = settle(&mut s).0.expect("a picture");
    let specular = maps.specular.as_ref().expect("specular");
    assert_eq!(picture.rgba[0], specular[0]);
    assert_eq!(picture.rgba[1], specular[0], "grey, from R");
    s.set_tab(Tab::Height);
    let picture = settle(&mut s).0.expect("a picture");
    let heights = maps.heights.as_ref().expect("heights");
    for (i, h) in heights.iter().enumerate() {
        assert_eq!(picture.rgba[i * 4], *h, "texel {i}");
    }
    s.set_tab(Tab::Normal);
    let picture = settle(&mut s).0.expect("a picture");
    let normals = maps.normals.as_ref().expect("normals");
    for (i, n) in normals.iter().enumerate() {
        let [_, r, g, b] = n.to_be_bytes();
        assert_eq!(&picture.rgba[i * 4..i * 4 + 3], &[r, g, b], "texel {i}");
    }
}

#[test]
fn a_drag_shows_a_draft_and_the_release_the_full_size() {
    let mut s = studio();
    set_page(&mut s, 512, PAGE);
    s.set_tab(Tab::Albedo);
    settle(&mut s);
    set_colour(&mut s, 1, [0.3, 0.3, 0.3], true);
    let end = Instant::now() + PATIENCE;
    let draft = loop {
        if let Some(picture) = s.tick().picture {
            break picture;
        }
        assert!(Instant::now() < end, "no draft");
        std::thread::sleep(Duration::from_millis(1));
    };
    assert_eq!((draft.width, draft.height, draft.full), (128, 128, false));
    set_colour(&mut s, 1, [0.3, 0.3, 0.3], false);
    let picture = settle(&mut s).0.expect("a picture");
    assert_eq!((picture.width, picture.full), (512, true));
    assert_eq!(picture.rgba, baked_rgba(&s));
}

#[test]
fn rapid_edits_leave_the_newest_picture() {
    let mut s = studio();
    set_page(&mut s, 256, PAGE);
    s.set_tab(Tab::Albedo);
    settle(&mut s);
    for i in 0..500 {
        let v = i as f32 / 500.0;
        set_colour(&mut s, 1, [v, 1.0 - v, 0.5], true);
        s.tick();
    }
    set_colour(&mut s, 1, [0.125, 0.75, 0.5], false);
    let picture = settle(&mut s).0.expect("a picture");
    assert_eq!(picture.rgba, baked_rgba(&s), "exactly the last edit");
    // Nothing older comes along after it.
    for _ in 0..50 {
        let tick = s.tick();
        assert!(tick.picture.is_none());
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn moving_to_another_material_shows_nothing_of_the_last() {
    let mut s = studio();
    set_page(&mut s, 256, PAGE);
    s.set_tab(Tab::Albedo);
    s.tick();
    s.create("other", 16).expect("creates");
    let tick = s.tick();
    assert!(tick.cleared);
    let picture = settle(&mut s).0.expect("a picture");
    assert_eq!(picture.width, 16);
}

#[test]
fn every_material_stays_where_it_was_left() {
    let mut s = studio();
    s.create("other", 16).expect("creates");
    s.edit(|d| {
        d.set_gloss(5, false);
        Ok(())
    });
    s.open_material("brick");
    s.open_material("other");
    assert_eq!(s.doc().expect("open").recipe().gloss, 5);
    assert!(
        s.rows()
            .iter()
            .any(|r| r.name == "other" && r.dirty && r.current)
    );
}

#[test]
fn names_follow_the_manifests_rules_and_cannot_be_taken() {
    let mut s = studio();
    for bad in [
        "", "A", "a b", "a_n", "-a", "a-", "con", "../x", "retired", "brick", "test",
    ] {
        assert!(s.create(bad, 64).is_err(), "{bad:?}");
    }
    assert!(s.create("fine", 63).is_err(), "not a power of two");
    assert!(s.create("fine", 2048).is_err(), "too large");
    assert!(s.create("fine", 64).is_ok());
}

/// Waits for Apply or Save to finish; its message.
pub(super) fn wait_bake(s: &mut TexStudio) -> String {
    let end = Instant::now() + PATIENCE;
    let mut said = String::new();
    while s.is_baking() || said.is_empty() {
        if let Some(m) = s.tick().message {
            said = m;
        }
        assert!(Instant::now() < end, "never finished");
        std::thread::sleep(Duration::from_millis(1));
    }
    said
}

#[test]
fn apply_installs_the_material_as_a_new_generation() {
    let mut s = studio();
    set_page(
        &mut s,
        32,
        "op 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\n",
    );
    s.edit(|d| {
        d.set_gloss(41, false);
        Ok(())
    });
    let generation = s.store.generation();
    assert!(s.apply());
    assert_eq!(wait_bake(&mut s), "Applied test");
    assert_eq!(s.store.generation(), generation + 1);
    let table = s.table().expect("table");
    let id = table.find("test").expect("in the table");
    assert_eq!(id, MaterialId(5), "past the stock ids");
    let material = table.get(id).expect("there");
    assert_eq!(material.texture.texel(0, 0), 0xffff_0000);
    assert!(material.texture.is_filtered());
    assert!(s.doc().expect("open").dirty(), "applied is not saved");
}

#[test]
fn apply_replaces_a_material_in_its_slot_and_the_gloss_goes_with_it() {
    let mut s = studio();
    s.open_material("metal");
    s.edit(|d| {
        d.set_gloss(123, false);
        Ok(())
    });
    assert!(s.apply());
    wait_bake(&mut s);
    let table = s.table().expect("table");
    assert_eq!(table.find("metal"), Some(MaterialId(3)));
    assert_eq!(table.get(MaterialId(3)).expect("there").gloss, 123);
    assert_eq!(table.len(), NAMES.len());
}

#[test]
fn a_page_with_an_error_is_not_applied_and_nothing_is_lost() {
    let mut s = studio();
    set_page(&mut s, 32, "op 0 0 1 flat\nop 5 5 1 out-albedo\n");
    let generation = s.store.generation();
    assert!(!s.apply());
    assert!(s.tick().message.expect("says why").contains("error"));
    assert_eq!(s.save(), SaveStart::Refused);
    assert_eq!(s.store.generation(), generation);
}

#[test]
fn a_page_with_no_albedo_output_says_so_and_changes_nothing() {
    let mut s = studio();
    set_page(&mut s, 32, "op 0 0 1 flat\n");
    let generation = s.store.generation();
    assert!(s.apply());
    assert!(wait_bake(&mut s).contains("out-albedo"));
    assert_eq!(s.store.generation(), generation);
}

fn stock_dir(name: &str) -> std::path::PathBuf {
    dir_of(name, &NAMES)
}

/// A directory of these materials, ids in order, each a flat recipe.
pub(super) fn dir_of(tag: &str, names: &[&str]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mscent-texstudio-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    let rows: String = names
        .iter()
        .enumerate()
        .map(|(i, n)| format!("{i} {n} 40\n"))
        .collect();
    std::fs::write(d.join("materials.txt"), rows).expect("manifest");
    for n in names {
        let text = "miniscent-recipe 1\nsize 16 16\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";
        std::fs::write(d.join(format!("{n}.mst")), text).expect("recipe");
    }
    d
}

#[test]
fn a_saved_material_loads_with_the_id_and_the_pixels_the_table_has() {
    let mut s = studio();
    let d = stock_dir("loads");
    s.store.set_dir(d.clone());
    set_page(&mut s, 32, PAGE);
    s.edit(|d| {
        d.set_gloss(77, false);
        Ok(())
    });
    assert_eq!(s.save(), SaveStart::Started);
    assert!(wait_bake(&mut s).starts_with("Saved test (id 5)"));
    assert!(!s.doc().expect("open").dirty(), "what is on disk is this");
    let loaded = MaterialTable::load_dir(&d).expect("the game can load it");
    let on_disk = loaded.get(MaterialId(5)).expect("id 5");
    let installed = s.table().expect("table");
    let here = installed.get(MaterialId(5)).expect("id 5");
    assert_eq!(on_disk.name, "test");
    assert_eq!((on_disk.gloss, here.gloss), (77, 77));
    for y in 0..32 {
        for x in 0..32 {
            assert_eq!(on_disk.texture.texel(x, y), here.texture.texel(x, y));
            assert_eq!(
                on_disk.texture.normal_texel(x, y),
                here.texture.normal_texel(x, y)
            );
        }
    }
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_failed_save_says_so_and_loses_nothing() {
    let mut s = studio();
    let d = stock_dir("fails");
    s.store.set_dir(d.clone());
    // Something is where the recipe must go.
    std::fs::create_dir(d.join("test.mst")).expect("in the way");
    set_page(
        &mut s,
        16,
        "op 0 0 1 flat color=0,1,0\nop 0 1 1 out-albedo\n",
    );
    s.edit(|doc| {
        doc.set_gloss(9, false);
        Ok(())
    });
    let generation = s.store.generation();
    assert_eq!(s.save(), SaveStart::Started);
    assert!(wait_bake(&mut s).starts_with("not saved"));
    let doc = s.doc().expect("open");
    assert!(doc.dirty(), "still to be saved");
    assert_eq!(doc.recipe().gloss, 9);
    assert_eq!(s.store.generation(), generation, "the table did not change");
    let text = std::fs::read_to_string(d.join("materials.txt")).expect("reads");
    assert!(!text.contains("test"), "{text}");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn lit_frames_keep_coming_and_stop_when_the_workspace_goes() {
    let mut s = studio();
    let page = "op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 noise seed=2\nop 1 1 1 out-height normal_strength=12\n";
    set_page(&mut s, 32, page);
    s.set_tab(Tab::Lit);
    let first = settle(&mut s).0.expect("a picture").rgba;
    let end = Instant::now() + PATIENCE;
    loop {
        if s.tick().picture.is_some_and(|p| p.rgba != first) {
            break;
        }
        assert!(Instant::now() < end, "the light never moved");
        std::thread::sleep(Duration::from_millis(1));
    }
    s.set_open(false);
    assert!(s.needs_ticks(), "the worker has yet to hear");
    s.tick();
    assert!(!s.needs_ticks());
    // Whatever was on its way is gone within a few frames, then nothing comes.
    std::thread::sleep(Duration::from_millis(200));
    let worker = s.worker.as_ref().expect("worker");
    let _ = worker.take();
    std::thread::sleep(Duration::from_millis(200));
    let outbox = worker.take().expect("free");
    assert!(outbox.picture.is_none(), "still animating");
}
