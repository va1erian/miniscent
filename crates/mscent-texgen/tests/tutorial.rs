//! The tutorial stays true: its recipe bakes, and its table names every operator.

use mscent_texgen::{bake, ops, recipe};

fn tutorial() -> String {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/texture-studio-tutorial.md"
    );
    std::fs::read_to_string(path)
        .expect("the tutorial")
        .replace("\r\n", "\n")
}

/// The text of the page's only recipe block.
fn finished_recipe(page: &str) -> &str {
    let start = page.find("miniscent-recipe 1").expect("a recipe");
    let length = page[start..].find("```").expect("its end");
    &page[start..start + length]
}

#[test]
fn the_finished_recipe_bakes_every_map() {
    let page = tutorial();
    let r = recipe::read(finished_recipe(&page).as_bytes()).expect("reads");
    let maps = bake(&r, r.size).expect("bakes");
    assert_eq!((maps.width, maps.height, r.gloss), (128, 128, 60));
    let specular = maps.specular.expect("specular");
    assert!(specular.contains(&0), "gaps have no shine");
    assert!(specular.contains(&255), "tiles do");
    assert!(maps.heights.is_some() && maps.normals.is_some());
    let reds = || maps.albedo.iter().map(|p| (p >> 16) & 0xff);
    assert!(reds().max() > reds().min(), "the stone varies");
}

#[test]
fn the_table_names_every_operator() {
    let page = tutorial();
    let table = &page[page.find("## Operators").expect("a table")..];
    for op in ops::all() {
        assert!(
            table.contains(&format!("`{}`", op.name)),
            "{} is missing",
            op.name
        );
    }
}

#[test]
fn the_steps_name_only_operators_that_exist() {
    let page = tutorial();
    let table = &page[page.find("## Operators").expect("a table")..];
    for line in table.lines().filter(|l| l.starts_with("| ")).skip(2) {
        let names = line.split('|').nth(2).expect("a column");
        for name in names.split('`').skip(1).step_by(2) {
            assert!(ops::find(name).is_some(), "no operator {name}");
        }
    }
}
