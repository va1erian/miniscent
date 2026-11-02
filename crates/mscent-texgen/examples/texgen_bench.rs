//! What making textures costs: the stock recipes, as a game start would make
//! them, and one parameter change, as the editor would.
//!
//! `cargo run --release -p mscent-texgen --example texgen_bench`

use std::path::Path;
use std::time::Instant;

use mscent_texgen::graph::{self, Cache, RunOptions};
use mscent_texgen::ops::Value;
use mscent_texgen::{Recipe, Size, bake, recipe};

const STOCK: [&str; 10] = [
    "bark",
    "brick",
    "cap",
    "coral",
    "floor",
    "lightpanel",
    "metal",
    "rock",
    "stem",
    "wax",
];
const RUNS: usize = 20;

fn stock(name: &str) -> Recipe {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let bytes = std::fs::read(dir.join(format!("{name}.mst"))).expect("stock recipe");
    recipe::read(&bytes).expect("reads")
}

/// The best of some runs, in ms: the run the scheduler disturbed least.
fn best_ms(mut f: impl FnMut()) -> f64 {
    (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .fold(f64::INFINITY, f64::min)
}

/// Changes the first noise's seed, then runs with what the cache already has.
fn one_change(recipe: &mut Recipe, size: Size, cache: &mut Cache, seed: i32) {
    let block = recipe
        .blocks
        .iter_mut()
        .find(|b| b.spec.name == "noise")
        .expect("a noise block");
    let spec = block.spec;
    block
        .params
        .set(spec, "seed", Value::Int(seed))
        .expect("seed");
    let g = graph::compile(recipe).expect("compiles");
    let options = RunOptions {
        cache: Some(cache),
        ..Default::default()
    };
    graph::run(recipe, &g, size, options).expect("runs");
}

fn main() {
    let recipes: Vec<(&str, Recipe)> = STOCK.iter().map(|n| (*n, stock(n))).collect();
    let mut total = 0.0;
    for (name, r) in &recipes {
        let ms = best_ms(|| {
            bake(r, r.size).expect("bakes");
        });
        total += ms;
        println!("{name:<12} {ms:7.2} ms at 128 px");
    }
    println!("{:<12} {total:7.2} ms at 128 px", "all");
    let size = Size::new(256, 256).expect("size");
    let mut r = stock("brick");
    let mut cache = Cache::new(256 << 20);
    one_change(&mut r, size, &mut cache, 1000);
    let mut seed = 1000;
    let ms = best_ms(|| {
        seed += 1;
        one_change(&mut r, size, &mut cache, seed);
    });
    println!("one change   {ms:7.2} ms at 256 px, brick");
}
