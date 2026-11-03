//! What building an enemy model costs (M15.1), against what `estimated_ns` says,
//! since the estimate must stay above the build on a file made to be slow.
//!
//! `cargo run --release -p mscent-bench --example model_bench`

use std::fmt::Write;
use std::hint::black_box;
use std::time::Instant;

use mscent_world::model::{Blueprint, MAX_MODEL_NS, Model, build, estimated_ns, read};

const REPEATS: usize = 5;
const HEAD: &str = "miniscent-blueprint 1\nname a\nclass ship\nskin hull metal\n";

fn best_ms(b: &Blueprint) -> Option<f64> {
    let mut best = f64::MAX;
    for _ in 0..REPEATS {
        let t = Instant::now();
        black_box(build(b).ok()?);
        best = best.min(t.elapsed().as_secs_f64() * 1e3);
    }
    Some(best)
}

fn triangles(m: &Model, level: usize) -> usize {
    m.parts.iter().map(|p| p.lods[level].triangles.len()).sum()
}

fn row(name: &str, text: &str) {
    let b = match read(text.as_bytes()) {
        Ok(b) => b,
        Err(e) => return println!("{name:<24} refused at reading: {e}"),
    };
    let t = Instant::now();
    black_box(read(text.as_bytes()).ok());
    let read_us = t.elapsed().as_secs_f64() * 1e6;
    let estimate = estimated_ns(&b) as f64 / 1e6;
    let Some(ms) = best_ms(&b) else {
        return println!(
            "{name:<24} refused: estimate {estimate:.0} ms, limit {} ms",
            MAX_MODEL_NS / 1_000_000
        );
    };
    let Ok(m) = build(&b) else {
        return;
    };
    println!(
        "{name:<24} {:>5} B  read {read_us:>6.0} us  build {ms:>7.2} ms  estimate {estimate:>7.1} ms  x{:>5.1}  tris {:>5} {:>5} {:>5}",
        text.len(),
        estimate / ms,
        triangles(&m, 0),
        triangles(&m, 1),
        triangles(&m, 2),
    );
}

fn with_head(body: &str) -> String {
    format!("{HEAD}{body}")
}

/// `solids` boxes along a beam, with `cuts` notches when there are any.
fn beam(solids: usize, cuts: usize) -> String {
    let mut s = String::from("part p\n");
    for i in 0..solids {
        let x = i as f64 * 1.5 - 30.0;
        let _ = writeln!(s, "solid cube {x} {} 1 1 1 1 hull", (i % 3) as f64 * 0.5);
    }
    for i in 0..cuts {
        let x = i as f64 * 3.0 - 28.0;
        let _ = writeln!(s, "cut cube {x} 0 1.5 0.5 3 0.5");
    }
    with_head(&s)
}

/// A dense surface: a ball bitten all over, then smoothed `smooths` times.
fn lumps(cell: &str, radius: f64, smooths: usize) -> String {
    let mut s = format!("part p cell={cell}\nadd sphere 0 0 0 {radius} hull\n");
    for i in 0..60 {
        let a = f64::from(i) * 2.399;
        let z = 1.0 - f64::from(i) / 30.0;
        let r = (1.0 - z * z).sqrt();
        let (x, y) = (r * a.cos() * radius, r * a.sin() * radius);
        let _ = writeln!(s, "carve sphere {x} {y} {} {}", z * radius, radius * 0.18);
    }
    for _ in 0..smooths {
        let _ = writeln!(s, "smooth sphere 0 0 0 {radius}");
    }
    with_head(&s)
}

fn main() {
    println!("build is the best of {REPEATS}; over is the estimate divided by it");
    for (name, text) in [
        ("sentry", include_str!("../../../assets/enemies/sentry.msb")),
        ("dart", include_str!("../../../assets/enemies/dart.msb")),
        ("mantis", include_str!("../../../assets/enemies/mantis.msb")),
    ] {
        row(name, text);
    }
    for solids in [16, 32, 63] {
        row(&format!("{solids} solids, direct"), &beam(solids, 0));
    }
    for (solids, cuts) in [(4, 2), (8, 4), (16, 8), (32, 16), (48, 16)] {
        row(
            &format!("{solids} solids, {cuts} cuts"),
            &beam(solids, cuts),
        );
    }
    for (cell, radius, smooths) in [
        ("1", 16.0, 0),
        ("0.5", 8.0, 0),
        ("0.5", 12.0, 1),
        ("0.25", 6.0, 1),
        ("0.25", 8.0, 2),
        ("0.25", 8.0, 8),
    ] {
        let name = format!("lumps {cell} r{radius} s{smooths}");
        row(&name, &lumps(cell, radius, smooths));
    }
}
