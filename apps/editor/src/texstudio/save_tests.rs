use mscent_render::{MaterialId, MaterialTable};
use mscent_texgen::recipe::read;

use super::*;

const MANIFEST: &str = "# ids\n0 brick 25\n";

fn recipe() -> Recipe {
    read(b"miniscent-recipe 1\nsize 16 16\ngloss 77\nop 0 0 1 flat color=0,1,0\nop 0 1 1 out-albedo\n")
        .expect("reads")
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
fn a_new_material_loads_through_load_dir_with_the_next_id() {
    let d = dir("new");
    assert_eq!(save_material(&d, "concrete", &recipe(), None), Ok(1));
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
    save_material(&d, "concrete", &recipe(), None).expect("first");
    let mut changed = recipe();
    changed.gloss = 9;
    assert_eq!(save_material(&d, "concrete", &changed, Some(1)), Ok(1));
    let table = MaterialTable::load_dir(&d).expect("loads");
    assert_eq!(table.get(MaterialId(1)).expect("there").gloss, 9);
    assert_eq!(table.len(), 2);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_directory_with_no_manifest_gets_one_that_keeps_its_ids() {
    let d = dir("listing");
    std::fs::remove_file(d.join(manifest::FILE)).expect("removes");
    assert_eq!(save_material(&d, "a-new-one", &recipe(), None), Ok(1));
    let table = MaterialTable::load_dir(&d).expect("loads");
    assert_eq!(table.find("brick"), Some(MaterialId(0)));
    assert_eq!(table.find("a-new-one"), Some(MaterialId(1)));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_disagreement_about_the_id_writes_nothing() {
    let d = dir("id");
    let before = std::fs::read(d.join(manifest::FILE)).expect("reads");
    let got = save_material(&d, "concrete", &recipe(), Some(9));
    assert!(got.is_err_and(|e| e.contains("save the materials")));
    assert!(!d.join("concrete.mst").exists());
    assert_eq!(
        std::fs::read(d.join(manifest::FILE)).expect("reads"),
        before
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_name_that_would_leave_the_directory_writes_nothing() {
    let d = dir("names");
    for bad in ["../evil", "a/b", "a\\b", "CON", "con", "", "a.b", "a_n"] {
        assert!(save_material(&d, bad, &recipe(), None).is_err(), "{bad}");
    }
    assert_eq!(listing(&d), ["brick.mst", "materials.txt"]);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_recipe_that_cannot_be_written_fails_and_leaves_the_old_one() {
    let d = dir("blocked");
    let path = d.join("concrete.mst");
    std::fs::create_dir(&path).expect("something in the way");
    let got = save_material(&d, "concrete", &recipe(), None);
    assert!(got.is_err(), "{got:?}");
    let text = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert_eq!(text, MANIFEST, "no row for what was not written");
    assert_eq!(
        listing(&d),
        ["brick.mst", "concrete.mst", "materials.txt"],
        "no temp files left"
    );
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_manifest_that_cannot_be_written_takes_the_recipe_back() {
    let d = dir("no-row");
    // Something is where the manifest's temp file must go.
    let temp = d.join(format!("materials.tmp{}", std::process::id()));
    std::fs::create_dir(temp).expect("in the way");
    assert!(save_material(&d, "concrete", &recipe(), None).is_err());
    assert!(!d.join("concrete.mst").exists(), "a new recipe is removed");
    let old = std::fs::read(d.join("brick.mst")).expect("reads");
    let mut changed = recipe();
    changed.gloss = 1;
    assert!(save_material(&d, "brick", &changed, Some(0)).is_err());
    let now = std::fs::read(d.join("brick.mst")).expect("reads");
    assert_eq!(now, old, "an old one is put back");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_broken_manifest_is_refused_not_overwritten() {
    let d = dir("broken");
    std::fs::write(d.join(manifest::FILE), "0 brick nonsense\n").expect("writes");
    assert!(save_material(&d, "concrete", &recipe(), None).is_err());
    assert!(!d.join("concrete.mst").exists());
    let kept = std::fs::read_to_string(d.join(manifest::FILE)).expect("reads");
    assert_eq!(kept, "0 brick nonsense\n");
    let _ = std::fs::remove_dir_all(d);
}
