//! Where the costs in the operator tables come from, and what the limits buy: each
//! operator on its worst parameters against the cost it declares, then hostile pages
//! cut down to what the limit lets through, as a game start would make them.
//!
//! `cargo run --release -p mscent-texgen --example texgen_costs`

use std::fmt::Write;
use std::time::Instant;

use mscent_texgen::graph::{self, MAX_LIVE_BYTES, MAX_RECIPE_NS};
use mscent_texgen::image::Frame;
use mscent_texgen::ops::{self, MAX_POINTS, MAX_STROKES, Stroke};
use mscent_texgen::recipe::{self, MAX_BLOCKS, Recipe};
use mscent_texgen::{Image, Size, bake};

/// Each operator with the parameters that cost it most, some with close rivals.
const WORST: &[&str] = &[
    "flat",
    "noise octaves=1 freq_x=256 freq_y=256",
    "noise octaves=8 freq_x=256 freq_y=256 mode=ridged",
    "noise octaves=8 freq_x=1 freq_y=1 persistence=1",
    "checker cols=64 rows=64",
    "stripes cycles_x=64 cycles_y=63",
    "gradient cycles_x=16 cycles_y=15",
    "glow radius=1 power=7.77",
    "glow radius=1 power=0.1",
    "bricks rows=64 cols=64 mode=tint",
    "bricks rows=64 cols=64 mode=bevel bevel=0.5 mortar=0.25",
    "cells count=256 mode=distance",
    "cells count=256 mode=edge",
    "cells count=4 mode=id",
    "wavelet octaves=1 freq=64",
    "wavelet octaves=6 freq=64",
    "rect w=0.7 h=0.7 softness=0.5",
    "ellipse w=0.7 h=0.3 softness=0.5",
    "line x0=0.1 y0=0.2 x1=0.9 y1=0.7 width=0.5 softness=0.5",
    "dots count_x=64 count_y=64 radius=0.5 softness=0.5 jitter=1",
    "dots count_x=1 count_y=1 radius=0.5 softness=0.5 jitter=1",
    "scratches count=0",
    "scratches count=512 length=0.5 width=0.05",
    "paint",
    "text text=WWWWWWWWWWWWWWWWWWWWWWWWWWWWWWWW scale=0.005",
    "merge mode=blend",
    "merge mode=screen",
    "mask",
    "paste w=1 h=1",
    "blur radius=0.25",
    "blur radius=0.01",
    "color amount=3",
    "range in_low=0.2 in_high=0.8 gamma=2.3",
    "invert",
    "hscb hue=0.3 saturation=1.5 contrast=1.5 brightness=1.5",
    "sharpen radius=0.1 amount=2",
    "rotozoom turns=1 zoom_x=16 zoom_y=16 shift_x=0.3 shift_y=0.3",
    "twirl strength=8 radius=0.5",
    "bulge strength=1 radius=0.5",
    "distort amount=0.5",
    "bump strength=32",
    "cavity radius=0.25 amount=8",
    "slope strength=32",
];

fn side() -> Size {
    Size::new(1024, 1024).expect("size")
}

fn read_page(body: &str) -> Recipe {
    let text = format!("miniscent-recipe 1\nsize 1024 1024\n{body}");
    recipe::read(text.as_bytes()).expect("reads")
}

fn seconds(mut f: impl FnMut()) -> f64 {
    let start = Instant::now();
    f();
    start.elapsed().as_secs_f64()
}

/// The best of three bakes: the one the rest of the machine disturbed least.
fn quietest_bake(r: &Recipe) -> f64 {
    (0..3)
        .map(|_| {
            seconds(|| {
                bake(r, r.size).expect("bakes");
            })
        })
        .fold(f64::INFINITY, f64::min)
}

/// A varied picture to work on, since some kernels cost more on noise than on flat.
fn input(seed: u32) -> Image {
    let body = format!("op 0 0 1 noise freq_x=16 freq_y=16 seed={seed}\nop 0 1 1 out-albedo\n");
    let r = read_page(&body);
    let g = graph::compile(&r).expect("compiles");
    let out = graph::run(&r, &g, side(), Default::default()).expect("runs");
    out.image(1).expect("an image").as_ref().clone()
}

fn measure_operators() -> f64 {
    let pictures: Vec<Image> = (1..=8).map(input).collect();
    println!(
        "{:<62} {:>8} {:>8} {:>6}",
        "operator on 1024 px", "ns/tx", "declared", "ratio"
    );
    let mut worst: f64 = 0.0;
    for line in WORST {
        let r = read_page(&format!("op 0 0 1 {line}\n"));
        let block = &r.blocks[0];
        let inputs = if block.spec.name == "merge" {
            8
        } else {
            usize::from(block.spec.inputs.min)
        };
        let refs: Vec<&Image> = pictures.iter().take(inputs).collect();
        let time = (0..3)
            .map(|_| {
                seconds(|| {
                    ops::eval(block.spec, &block.params, &refs, Frame::new(side())).expect("runs");
                })
            })
            .fold(f64::INFINITY, f64::min);
        let measured = time * 1e9 / side().texels() as f64;
        let declared =
            ops::cost_ns(block.spec, &block.params, inputs, side()) as f64 / side().texels() as f64;
        let ratio = measured / declared;
        worst = worst.max(ratio);
        println!("{line:<62} {measured:>8.1} {declared:>8.1} {ratio:>6.2}");
    }
    worst
}

/// A page of `count` blocks of `what` as a run would meet them: generators nothing
/// reads, a chain of filters each on the last, or rows of a block over its inputs.
fn page_of(what: &str, count: usize) -> String {
    let name = what.split(' ').next().unwrap_or_default();
    let needs = ops::find(name).map_or(0, |op| match op.name {
        "merge" => 8,
        _ => usize::from(op.inputs.min),
    });
    let mut body = String::new();
    match needs {
        0 => {
            for i in 0..count {
                let _ = writeln!(body, "op {} {} 1 {what}", i % 64, i / 64);
            }
            body.push_str("op 0 200 1 flat\nop 0 201 1 out-albedo\n");
        }
        1 => {
            body.push_str("op 0 0 1 noise\n");
            for y in 1..=count {
                let _ = writeln!(body, "op 0 {y} 1 {what}");
            }
            let _ = writeln!(body, "op 0 {} 1 out-albedo", count + 1);
        }
        _ => {
            for i in 0..count {
                for x in 0..needs {
                    let _ = writeln!(body, "op {x} {} 1 noise octaves=1", i * 2);
                }
                let _ = writeln!(body, "op 0 {} {needs} {what}", i * 2 + 1);
            }
            let _ = writeln!(body, "op 60 0 1 flat\nop 60 1 1 out-albedo");
        }
    }
    body
}

/// Whether the plan takes this page.
fn accepted(r: &Recipe) -> bool {
    let g = graph::compile(r).expect("compiles");
    graph::page_cost(r, &g, r.size) <= MAX_RECIPE_NS
        && graph::peak_images(r, &g, &[]) * r.size.bytes() <= MAX_LIVE_BYTES
}

/// The most of a page the limits take, by bisection, then how long it takes. The
/// full hostile page is timed too, for how soon it is refused.
fn cut_down(name: &str, cap: usize, page: impl Fn(usize) -> String) -> f64 {
    let full = read_page(&page(cap));
    let g = graph::compile(&full).expect("compiles");
    let refused = seconds(|| {
        let _ = graph::plan::check(&full, &g, full.size, &[]);
    });
    let whole = accepted(&full);
    let (mut low, mut high) = (if whole { cap } else { 0 }, cap);
    while high - low > 1 {
        let middle = (low + high) / 2;
        if accepted(&read_page(&page(middle))) {
            low = middle;
        } else {
            high = middle;
        }
    }
    let r = read_page(&page(low));
    let g = graph::compile(&r).expect("compiles");
    let declared = graph::page_cost(&r, &g, r.size) as f64 / 1e9;
    let time = quietest_bake(&r);
    let verdict = if whole { "taken" } else { "refused" };
    println!(
        "{name:<62} {low:>3}/{cap} {time:>6.2} s, declared {declared:>5.2} s, ratio {:.2}; the {cap} {verdict} in {:.0} us",
        time / declared,
        refused * 1e6
    );
    time
}

fn measure_pages() {
    println!(
        "\nthe most of each page that fits, at 1024 px, against {} s:",
        MAX_RECIPE_NS as f64 / 1e9
    );
    let mut slowest: f64 = 0.0;
    for what in WORST {
        let name = what.split(' ').next().unwrap_or_default();
        let inputs = ops::find(name).map_or(0, |op| match op.name {
            "merge" => 8,
            _ => usize::from(op.inputs.min),
        });
        let cap = if inputs < 2 {
            254
        } else {
            MAX_BLOCKS / (inputs + 1) - 1
        };
        slowest = slowest.max(cut_down(what, cap, |n| page_of(what, n)));
    }
    slowest = slowest.max(cut_down(
        "paints of 170 big dabs (the 1.7 MB page)",
        254,
        |n| {
            let mut body = String::new();
            let dabs = "stroke 0.5 0.5 0.5,0.5,0.5,0.5 0.5,0.5\n".repeat(170);
            for i in 0..n {
                let _ = writeln!(body, "op {} {} 1 paint", i % 64, i / 64);
                body.push_str(&dabs);
            }
            body.push_str("op 0 200 1 flat\nop 0 201 1 out-albedo\n");
            body
        },
    ));
    println!("slowest page that passed: {slowest:.2} s");
}

/// A `paint` with all the strokes the rule takes of this shape: to and fro between
/// `a` and `b`, as many points in a stroke as can be.
fn full_paint(radius: f32, a: [f32; 2], b: [f32; 2]) -> String {
    let stroke_of = |points: usize| Stroke {
        radius,
        hardness: 0.5,
        color: [0.5, 0.5, 0.5, 0.5],
        points: (0..points)
            .map(|i| if i % 2 == 0 { a } else { b })
            .collect(),
    };
    let mut points = MAX_POINTS;
    while ops::charge_stroke(0.0, &stroke_of(points)).is_none() {
        points /= 2;
    }
    let stroke = stroke_of(points);
    let (mut work, mut count) = (0.0, 0);
    while count < MAX_STROKES {
        let Some(next) = ops::charge_stroke(work, &stroke) else {
            break;
        };
        work = next;
        count += 1;
    }
    let mut line = format!("stroke {radius} 0.5 0.5,0.5,0.5,0.5");
    for p in &stroke.points {
        let _ = write!(line, " {},{}", p[0], p[1]);
    }
    line.push('\n');
    println!("  {count} strokes of {points} points, {work:.0} tile units");
    let mut body = String::from("op 0 0 1 paint\n");
    for _ in 0..count {
        body.push_str(&line);
    }
    body.push_str("op 0 1 1 out-albedo\n");
    body
}

/// One `paint` at the most the rule allows, on each shape that could cost most. A
/// slope that keeps a segment in a row's band for its whole length costs most, so
/// each radius is tried at that slope too.
fn measure_paints() {
    println!("\none paint at the rule's limit, at 1024 px:");
    let at = |dy: f32| ([-2.0, 0.5], [3.0, 0.5 + dy]);
    let band = |radius: f32| 2.0 * (radius + 2.0 / 1024.0);
    let mut shapes: Vec<(String, f32, [f32; 2], [f32; 2])> = vec![
        ("big dabs r=0.5".into(), 0.5, [0.5, 0.5], [0.5, 0.5]),
        ("thick r=0.2, thinned".into(), 0.2, [0.5, 0.5], [0.55, 0.5]),
        (
            "thick r=0.2, short diagonals".into(),
            0.2,
            [0.5, 0.5],
            [0.6, 0.6],
        ),
        (
            "thin long diagonal r=0".into(),
            0.0,
            [-2.0, -2.0],
            [3.0, 3.0],
        ),
        ("thin long level r=0".into(), 0.0, at(0.0).0, at(0.0).1),
    ];
    for radius in [0.0, 0.002, 0.01, 0.05, 0.2] {
        for (name, slope) in [("worst", 1.0), ("half that", 0.5), ("twice that", 2.0)] {
            let (a, b) = at(band(radius) * slope);
            shapes.push((format!("r={radius} long, {name} slope"), radius, a, b));
        }
    }
    let mut slowest: f64 = 0.0;
    for (name, radius, a, b) in shapes {
        println!("{name}");
        let r = read_page(&full_paint(radius, a, b));
        let g = graph::compile(&r).expect("compiles");
        let declared = graph::page_cost(&r, &g, r.size) as f64 / 1e9;
        let time = quietest_bake(&r);
        slowest = slowest.max(time);
        println!(
            "  {time:.2} s, declared {declared:.2} s, ratio {:.2}",
            time / declared
        );
    }
    println!("slowest paint: {slowest:.2} s");
}

/// How long the costliest kinds of file take to read, per byte.
fn measure_reading() {
    println!("\nreading files of near {} MiB:", recipe::MAX_FILE >> 20);
    let header = "miniscent-recipe 1\nsize 4 4\n";
    let short_points = format!("stroke 0 1 0,0,0,0{}\n", " 1,1".repeat(MAX_POINTS));
    let long_points = format!(
        "stroke 0 1 0,0,0,0{}\n",
        " 0.123456,0.654321".repeat(MAX_POINTS)
    );
    let tiny = format!("stroke 0 1 0,0,0,0{}\n", " 1e-5,1e-5".repeat(MAX_POINTS));
    let many = format!("stroke 0 1 0,0,0,0{}\n", " 0.5,0.5".repeat(120));
    let short_strokes = "stroke 0 1 0,0,0,0 1,1\n".to_string();
    let op = "op 0 0 1 noise freq_x=4 freq_y=4 octaves=4\n".to_string();
    let comment = "#\n".to_string();
    let cases: [(&str, &str, &str, usize); 7] = [
        ("one-digit points", "op 0 0 1 paint\n", &short_points, 239),
        ("six-digit points", "op 0 0 1 paint\n", &long_points, 53),
        ("short exponents", "op 0 0 1 paint\n", &tiny, 100),
        (
            "120 points a stroke",
            "op 0 0 1 paint\n",
            &many,
            MAX_STROKES,
        ),
        (
            "short strokes",
            "op 0 0 1 paint\n",
            &short_strokes,
            MAX_STROKES,
        ),
        ("ops", "", &op, 255),
        ("empty comments", "", &comment, 2_000_000),
    ];
    for (name, start, line, count) in cases {
        let text = format!("{header}{start}{}", line.repeat(count));
        let read_ok = recipe::read(text.as_bytes()).is_ok();
        let time = (0..3)
            .map(|_| {
                seconds(|| {
                    let _ = recipe::read(text.as_bytes());
                })
            })
            .fold(f64::INFINITY, f64::min);
        let per_byte = time * 1e9 / text.len() as f64;
        println!(
            "{name:<24} {:>8} bytes {:>8.1} ms {per_byte:>6.1} ns/byte, reads: {read_ok}",
            text.len(),
            time * 1e3
        );
    }
}

/// With an argument, only the pages whose operator lines contain it, to measure
/// one operator again without the rest.
fn main() {
    if let Some(only) = std::env::args().nth(1) {
        for what in WORST.iter().filter(|w| w.contains(&only)) {
            cut_down(what, 254, |n| page_of(what, n));
        }
        return;
    }
    let worst = measure_operators();
    println!("worst ratio of measured to declared: {worst:.2}");
    measure_reading();
    measure_paints();
    measure_pages();
}
