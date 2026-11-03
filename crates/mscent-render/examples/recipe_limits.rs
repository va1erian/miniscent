//! What the limit on a directory of recipes buys: the most a manifest can ask for,
//! timed as a game start would make it, and where the cost of packing maps and
//! reading files comes from.
//!
//! `cargo run --release -p mscent-render --example recipe_limits`

use std::path::{Path, PathBuf};
use std::time::Instant;

use mscent_render::manifest::Entry;
use mscent_render::recipe_texture::{MAX_DIRECTORY_NS, check_directory_work, load_recipe};
use mscent_render::{MaterialTable, TextureError};
use mscent_texgen::recipe::MAX_FILE;

const HEADER: &str = "miniscent-recipe 1\nsize 1024 1024\n";

fn seconds(mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64()
}

fn fresh_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mscent-limits-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

/// `rows` copies of a recipe, and a manifest that lists them.
fn write_dir(dir: &Path, rows: usize, recipe: &str) -> Vec<Entry> {
    let mut manifest = String::new();
    let mut entries = Vec::new();
    for row in 0..rows {
        let name = format!("m{row}");
        std::fs::write(dir.join(format!("{name}.mst")), recipe).expect("write");
        manifest.push_str(&format!("{row} {name} 1\n"));
        entries.push(Entry::Material {
            name,
            gloss: 1,
            depth: None,
        });
    }
    std::fs::write(dir.join("materials.txt"), manifest).expect("write");
    entries
}

/// Rows of the dearest recipe there can be, as many as the limit takes, and one more.
fn directory_at_the_limit() {
    let dir = fresh_dir("at-limit");
    let mut recipe = String::from(HEADER);
    recipe.push_str("op 0 0 1 noise\n");
    let blocks = 33;
    for y in 1..=blocks {
        recipe.push_str(&format!("op 0 {y} 1 cavity radius=0.25 amount=8\n"));
    }
    recipe.push_str(&format!("op 0 {} 1 out-albedo\n", blocks + 1));
    let mut rows = 1;
    loop {
        let entries = write_dir(&dir, rows, &recipe);
        if check_directory_work(&dir, &entries).is_err() {
            break;
        }
        rows += 1;
    }
    let too_many = rows;
    let refused = seconds(|| {
        let got = MaterialTable::load_dir(&dir);
        assert!(matches!(got, Err(TextureError::Recipe(_))), "{got:?}");
    });
    println!(
        "{too_many} rows of about 5 s are refused in {:.1} ms",
        refused * 1e3
    );
    std::fs::remove_file(dir.join(format!("m{}.mst", too_many - 1))).expect("remove");
    let accepted = too_many - 1;
    write_dir(&dir, accepted, &recipe);
    let time = seconds(|| {
        MaterialTable::load_dir(&dir).expect("loads");
    });
    println!(
        "{accepted} rows of about 5 s, the most that pass: {time:.1} s against {} s declared at most",
        MAX_DIRECTORY_NS as f64 / 1e9
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// The cheapest kernels at the largest size, so packing is most of the time.
fn packing() {
    let dir = fresh_dir("pack");
    let one = format!("{HEADER}op 0 0 1 flat\nop 0 1 1 out-albedo\n");
    let three = format!(
        "{HEADER}op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-height\nop 2 0 1 flat\nop 2 1 1 out-specular\n"
    );
    for (name, text, maps, flats) in [("one", &one, 1.0, 1.0), ("three", &three, 3.0, 3.0)] {
        let path = dir.join(format!("{name}.mst"));
        std::fs::write(&path, text).expect("write");
        let time = (0..5)
            .map(|_| {
                seconds(|| {
                    load_recipe(&path).expect("loads");
                })
            })
            .fold(f64::INFINITY, f64::min);
        let per_map = (time * 1e9 / (1024.0 * 1024.0) - flats * 2.0) / maps;
        println!(
            "{name} map(s): {:.1} ms, {per_map:.1} ns/texel a map",
            time * 1e3
        );
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// Rows that are mostly packing or mostly reading, sampled: a full manifest of them
/// is gigabytes of disk or memory for the same answer.
fn cheap_rows() {
    let dir = fresh_dir("cheap");
    let flat = format!("{HEADER}op 0 0 1 flat\nop 0 1 1 out-albedo\n");
    let rows = 64;
    write_dir(&dir, rows, &flat);
    let time = seconds(|| {
        MaterialTable::load_dir(&dir).expect("loads");
    });
    println!(
        "{rows} rows of a 1024 px flat: {time:.2} s, {:.1} ms a row",
        time * 1e3 / rows as f64
    );
    let _ = std::fs::remove_dir_all(&dir);

    let dir = fresh_dir("big-files");
    let mut big = String::from("miniscent-recipe 1\nsize 4 4\nop 0 0 1 paint\n");
    let line = format!("stroke 0 1 0,0,0,0{}\n", " 1,1".repeat(4096));
    while big.len() + line.len() < MAX_FILE - 64 {
        big.push_str(&line);
    }
    big.push_str("op 0 1 1 out-albedo\n");
    let rows = 16;
    write_dir(&dir, rows, &big);
    let time = seconds(|| {
        MaterialTable::load_dir(&dir).expect("loads");
    });
    println!(
        "{rows} rows of a file near 4 MiB at 4 px: {time:.2} s, {:.0} ms a row",
        time * 1e3 / rows as f64
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// What the stock directory declares, against the limit.
fn stock() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let mut declared = 0;
    for name in mscent_world::materials::NAMES {
        let bytes = std::fs::read(dir.join(format!("{name}.mst"))).expect("stock recipe");
        let recipe = mscent_texgen::recipe::read(&bytes).expect("reads");
        declared += mscent_texgen::pack::bake_cost(&recipe, recipe.size).expect("costs");
    }
    let time = seconds(|| {
        MaterialTable::load_dir(&dir).expect("loads");
    });
    println!(
        "stock: {:.1} ms declared, {:.1} ms measured, of {} s",
        declared as f64 / 1e6,
        time * 1e3,
        MAX_DIRECTORY_NS as f64 / 1e9
    );
}

fn main() {
    stock();
    packing();
    cheap_rows();
    directory_at_the_limit();
}
