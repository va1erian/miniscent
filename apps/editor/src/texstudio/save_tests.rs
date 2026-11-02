use mscent_render::{Material, MaterialId, MaterialTable, Texture};
use mscent_texgen::recipe::read;

use super::*;

const MANIFEST: &str = "# ids\n0 brick 25\n";

fn recipe() -> Recipe {
    read(b"miniscent-recipe 1\nsize 16 16\ngloss 77\nop 0 0 1 flat color=0,1,0\nop 0 1 1 out-albedo\n")
        .expect("reads")
}

/// A running table holding these materials, by id.
fn table_of(names: &[&str]) -> MaterialTable {
    let mut table = MaterialTable::new();
    for name in names {
        table.add(Material {
            name: name.to_string(),
            gloss: 40,
            texture: Texture::new(1, 1, vec![0xff00_0000]).expect("texture"),
        });
    }
    table
}

/// The table of a directory made by [`dir`].
fn brick() -> MaterialTable {
    table_of(&["brick"])
}

/// A directory with a manifest and a recipe for its one material.
fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mscent-save-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    std::fs::write(d.join(manifest::FILE), MANIFEST).expect("manifest");
    std::fs::write(d.join("brick.mst"), recipe::write(&recipe())).expect("recipe");
    d
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("reads")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_page_that_would_push_the_directory_past_its_limit_is_refused_and_writes_nothing() {
    let d = dir("limit");
    let big = "miniscent-recipe 1\nsize 1024 1024\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";
    let mut text = MANIFEST.to_string();
    let mut names = vec!["brick".to_string()];
    for i in 1..=63 {
        let name = format!("big{i}");
        std::fs::write(d.join(format!("{name}.mst")), big).expect("recipe");
        text.push_str(&format!("{i} {name} 40\n"));
        names.push(name);
    }
    std::fs::write(d.join(manifest::FILE), &text).expect("manifest");
    let table = table_of(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let page = read(big.as_bytes()).expect("reads");
    let before = listing(&d);
    let refused = save_material(&d, "onemore", &page, &table).expect_err("refused");
    assert!(refused.contains("too much"), "{refused}");
    assert_eq!(listing(&d), before);
    let after = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert_eq!(after, text);
}

#[test]
fn a_new_material_loads_through_load_dir_with_the_next_id() {
    let d = dir("new");
    assert_eq!(save_material(&d, "concrete", &recipe(), &brick()), Ok(1));
    let table = MaterialTable::load_dir(&d).expect("loads");
    assert_eq!(table.find("concrete"), Some(MaterialId(1)));
    let material = table.get(MaterialId(1)).expect("there");
    assert_eq!(material.gloss, 77);
    assert_eq!(material.texture.texel(3, 3), 0xff00_ff00);
    let text = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert!(text.starts_with("# ids\n"), "the comment stays: {text}");
    assert_eq!(
        listing(&d),
        ["brick.mst", "concrete.mst", "materials.txt"],
        "no temp files left"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn saving_again_keeps_the_id_and_takes_the_new_gloss() {
    let d = dir("again");
    save_material(&d, "concrete", &recipe(), &brick()).expect("first");
    let mut changed = recipe();
    changed.gloss = 9;
    let both = table_of(&["brick", "concrete"]);
    assert_eq!(save_material(&d, "concrete", &changed, &both), Ok(1));
    let table = MaterialTable::load_dir(&d).expect("loads");
    assert_eq!(table.get(MaterialId(1)).expect("there").gloss, 9);
    assert_eq!(table.len(), 2);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_directory_with_no_manifest_gets_one_that_keeps_its_ids() {
    let d = dir("listing");
    std::fs::remove_file(d.join(manifest::FILE)).expect("removes");
    assert_eq!(save_material(&d, "a-new-one", &recipe(), &brick()), Ok(1));
    let table = MaterialTable::load_dir(&d).expect("loads");
    assert_eq!(table.find("brick"), Some(MaterialId(0)));
    assert_eq!(table.find("a-new-one"), Some(MaterialId(1)));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_disagreement_about_the_id_writes_nothing_and_names_what_to_save_first() {
    let d = dir("id");
    let before = std::fs::read(d.join(manifest::FILE)).expect("reads");
    // "alpha" was applied and not saved, so it has the id this one would take.
    let running = table_of(&["brick", "alpha"]);
    let got = save_material(&d, "beta", &recipe(), &running);
    assert!(got.is_err_and(|e| e.contains("save alpha first")));
    assert!(!d.join("beta.mst").exists());
    assert_eq!(
        std::fs::read(d.join(manifest::FILE)).expect("reads"),
        before
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_row_added_by_another_process_is_a_disagreement_not_a_loss() {
    let d = dir("other-process");
    std::fs::write(d.join(manifest::FILE), "0 brick 25\n1 theirs 40\n").expect("writes");
    let got = save_material(&d, "mine", &recipe(), &brick());
    assert!(got.is_err_and(|e| e.contains("changed since")), "refused");
    assert!(!d.join("mine.mst").exists());
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_new_name_over_an_unlisted_recipe_is_refused_and_the_file_is_untouched() {
    let d = dir("unlisted");
    std::fs::write(d.join("foo.mst"), b"someone's").expect("writes");
    let got = save_material(&d, "foo", &recipe(), &brick());
    assert!(got.is_err_and(|e| e.contains("does not list")));
    assert_eq!(
        std::fs::read(d.join("foo.mst")).expect("reads"),
        b"someone's"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_name_that_would_leave_the_directory_writes_nothing() {
    let d = dir("names");
    for bad in ["../evil", "a/b", "a\\b", "CON", "con", "", "a.b", "a_n"] {
        assert!(
            save_material(&d, bad, &recipe(), &brick()).is_err(),
            "{bad}"
        );
    }
    assert_eq!(listing(&d), ["brick.mst", "materials.txt"]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_recipe_that_cannot_be_written_fails_and_leaves_the_old_one() {
    let d = dir("blocked");
    // The recipe's own file is listed but cannot be replaced: a directory is there.
    std::fs::write(d.join(manifest::FILE), "0 brick 25\n1 concrete 40\n").expect("manifest");
    let path = d.join("concrete.mst");
    std::fs::create_dir(&path).expect("something in the way");
    let running = table_of(&["brick", "concrete"]);
    let got = save_material(&d, "concrete", &recipe(), &running);
    assert!(got.is_err(), "{got:?}");
    let text = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert_eq!(
        text, "0 brick 25\n1 concrete 40\n",
        "the manifest is as it was"
    );
    assert_eq!(
        listing(&d),
        ["brick.mst", "concrete.mst", "materials.txt"],
        "no temp files left"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_recipe_that_cannot_be_read_back_is_not_taken_for_a_missing_one() {
    let d = dir("unreadable");
    // A directory where the listed recipe should be: reading it fails, but it is
    // not "no old file", so nothing may be written and nothing deleted.
    std::fs::remove_file(d.join("brick.mst")).expect("removes");
    std::fs::create_dir(d.join("brick.mst")).expect("in the way");
    let manifest_before = std::fs::read(d.join(manifest::FILE)).expect("reads");
    assert!(save_material(&d, "brick", &recipe(), &brick()).is_err());
    assert!(d.join("brick.mst").is_dir(), "still what it was");
    assert_eq!(
        std::fs::read(d.join(manifest::FILE)).expect("reads"),
        manifest_before
    );
    assert_eq!(listing(&d), ["brick.mst", "materials.txt"]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_manifest_that_cannot_be_written_takes_the_recipe_back() {
    let d = dir("no-row");
    // Something is where the manifest's temp file must go.
    let temp = d.join(format!("materials.tmp{}", std::process::id()));
    std::fs::create_dir(temp).expect("in the way");
    assert!(save_material(&d, "concrete", &recipe(), &brick()).is_err());
    assert!(!d.join("concrete.mst").exists(), "a new recipe is removed");
    let old = std::fs::read(d.join("brick.mst")).expect("reads");
    let mut changed = recipe();
    changed.gloss = 1;
    assert!(save_material(&d, "brick", &changed, &brick()).is_err());
    let now = std::fs::read(d.join("brick.mst")).expect("reads");
    assert_eq!(now, old, "an old one is put back");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_broken_manifest_is_refused_not_overwritten() {
    let d = dir("broken");
    std::fs::write(d.join(manifest::FILE), "0 brick nonsense\n").expect("writes");
    assert!(save_material(&d, "concrete", &recipe(), &brick()).is_err());
    assert!(!d.join("concrete.mst").exists());
    let kept = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert_eq!(kept, "0 brick nonsense\n");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_written_file_replaces_the_old_whole_and_leaves_no_temp() {
    let d = dir("atomic");
    let path = d.join("x.txt");
    write_atomic(&path, b"first").expect("writes");
    write_atomic(&path, b"second").expect("replaces");
    assert_eq!(std::fs::read(&path).expect("reads"), b"second");
    assert_eq!(listing(&d), ["brick.mst", "materials.txt", "x.txt"]);
    let _ = std::fs::remove_dir_all(d);
}
