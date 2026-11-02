use super::*;
use mscent_math::rng::Rng;

const SMALL: &str = "miniscent-recipe 1
size 64 32
gloss 25
# a comment, and a blank line

op 0 0 1 noise freq_x=8 persistence=0.25 mode=ridged color1=1,0.5,0
op 1 0 1 flat color=0.5,0.5,0.5,0.25
op 0 1 2 merge mode=multiply
op 0 2 1 store name=a%20b
op 0 3 1 out-albedo
";

#[test]
fn it_reads_what_the_format_says() {
    let r = read(SMALL.as_bytes()).expect("reads");
    assert_eq!((r.size.width(), r.size.height(), r.gloss), (64, 32, 25));
    assert_eq!(r.blocks.len(), 5);
    let noise = &r.blocks[0];
    assert_eq!(
        (noise.x, noise.y, noise.w, noise.spec.name),
        (0, 0, 1, "noise")
    );
    assert_eq!(r.blocks[2].w, 2);
    let values = r.blocks[3].params.values();
    assert_eq!(values[0], Value::Text("a b".to_string()));
}

#[test]
fn what_it_writes_reads_back_the_same() {
    let r = read(SMALL.as_bytes()).expect("reads");
    let text = write(&r);
    assert_eq!(read(text.as_bytes()).expect("reads back"), r);
    assert!(!text.contains("octaves"), "defaults are left out:\n{text}");
}

#[test]
fn awkward_text_survives_the_round_trip() {
    let store = ops::find("store").expect("store");
    let mut params = Params::defaults(store);
    let name = "a b%c=d#e\u{e9}\t";
    params
        .set(store, "name", Value::Text(name.to_string()))
        .expect("text");
    let r = Recipe {
        size: Size::new(8, 8).expect("size"),
        gloss: 1,
        blocks: vec![Block {
            x: 0,
            y: 0,
            w: 1,
            spec: store,
            params,
        }],
    };
    assert_eq!(read(write(&r).as_bytes()).expect("reads"), r);
}

#[test]
fn hostile_files_are_errors() {
    let cases = [
        "".to_string(),
        "miniscent-recipe 2\nsize 8 8\n".to_string(),
        "miniscent-recipe 1\n".to_string(),
        "miniscent-recipe 1\nsize 3 8\n".to_string(),
        "miniscent-recipe 1\nsize 8 99999999\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\ngloss 256\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 nosuchop\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 99999 0 1 flat\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat nope=1\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=2,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=1,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat color=NaN,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise octaves=999\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise persistence=inf\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 noise mode=wrong\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name=%zz\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name=%ff\n".to_string(),
        format!(
            "miniscent-recipe 1\nsize 8 8\nop 0 0 1 store name={}\n",
            "a".repeat(65)
        ),
        "miniscent-recipe 1\nsize 8 8\nstroke 0 0 0,0,0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nop 0 0 1 flat\nstroke 0.1 0 0,0,0,0 0,0\n".to_string(),
        "miniscent-recipe 1\nsize 8 8\nsomething else\n".to_string(),
    ];
    for case in cases {
        assert!(read(case.as_bytes()).is_err(), "accepted: {case:?}");
    }
    assert!(read(&[0xff, 0xfe, 0x00]).is_err());
    assert!(read(&vec![b' '; MAX_FILE + 1]).is_err());
}

#[test]
fn too_many_blocks_is_an_error() {
    let mut text = String::from("miniscent-recipe 1\nsize 8 8\n");
    for i in 0..=MAX_BLOCKS {
        text.push_str(&format!("op {i} 0 1 flat\n"));
    }
    assert!(read(text.as_bytes()).is_err());
}

#[test]
fn bit_flips_and_truncations_never_panic() {
    let bytes = SMALL.as_bytes();
    for cut in 0..bytes.len() {
        let _ = read(&bytes[..cut]);
    }
    let mut rng = Rng::new(0x07ec_19e5);
    for _ in 0..20_000 {
        let mut b = bytes.to_vec();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(b.len() as u32) as usize;
            b[at] ^= 1 << rng.below(8);
        }
        let _ = read(&b);
    }
}

/// Flipped bits in stroke lines, which carry the most numbers, never panic, and
/// whatever still reads is a stroke the operator would run.
#[test]
fn bit_flips_in_stroke_lines_never_panic_and_what_reads_is_drawable() {
    let text = painted(
        "stroke 0.03 0.5 1,0.5,0.25,1 -0.25,0.5 0.5,0.5 1.125,0.75\nstroke 0.2 0 0,0,0,0.5 2,2",
    );
    let bytes = text.as_bytes();
    let mut rng = Rng::new(0x5712_0be5);
    for _ in 0..20_000 {
        let mut b = bytes.to_vec();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(b.len() as u32) as usize;
            b[at] ^= 1 << rng.below(8);
        }
        let Ok(r) = read(&b) else { continue };
        for block in &r.blocks {
            for value in block.params.values() {
                if let Value::Strokes(list) = value {
                    assert!(list.iter().all(ops::stroke_fits));
                }
            }
        }
    }
}

fn painted(stroke_lines: &str) -> String {
    format!("miniscent-recipe 1\nsize 8 8\nop 0 0 1 paint\n{stroke_lines}\n")
}

#[test]
fn strokes_survive_the_round_trip() {
    let text = painted(
        "stroke 0.03 0.5 1,0.5,0.25,1 -0.25,0.5 0.5,0.5 1.125,0.75\nstroke 0.5 0 0,0,0,0.5 2.9,-1.9",
    );
    let r = read(text.as_bytes()).expect("reads");
    assert_eq!(read(write(&r).as_bytes()).expect("reads back"), r);
    let Some(Value::Strokes(s)) = r.blocks[0].params.values().get(1) else {
        panic!("a strokes value");
    };
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].points, [[-0.25, 0.5], [0.5, 0.5], [1.125, 0.75]]);
}

/// The reader takes what the kernel takes and no more, at each end of each range.
#[test]
fn the_reader_accepts_exactly_what_paint_runs() {
    for line in ["stroke 0.5 1 1,1,1,1 -2,3", "stroke 0 0 0,0,0,0 3,-2"] {
        assert!(read(painted(line).as_bytes()).is_ok(), "{line}");
    }
    let past = [
        "stroke 0.51 1 1,1,1,1 0,0",
        "stroke 1 1 1,1,1,1 0,0",
        "stroke 0.1 1.5 1,1,1,1 0,0",
        "stroke 0.1 1 1,1,1,2 0,0",
        "stroke 0.1 1 1,1,1,1 3.01,0",
        "stroke 0.1 1 1,1,1,1 0,-2.01",
        "stroke 0.1 1 1,1,1,1 16,0",
        "stroke NaN 1 1,1,1,1 0,0",
        "stroke 0.1 1 1,1,1,1 inf,0",
        "stroke 0.1 1 1,1,1,1 0",
        "stroke 0.1 1 1,1,1,1 0,0,0",
    ];
    for line in past {
        assert!(read(painted(line).as_bytes()).is_err(), "{line}");
    }
}

/// The tiniest negative `f32` prints the longest of anything in a stroke's range.
fn longest_coordinate() -> f32 {
    -f32::from_bits(1)
}

/// Any stroke the operator takes fits one line of a file: this is the longest one
/// there can be, with every number as long as it prints, and it comes back the same.
#[test]
fn a_stroke_of_the_longest_coordinates_at_the_point_cap_is_a_line_of_a_file() {
    let tiny = longest_coordinate();
    assert_eq!(tiny.to_string().len(), LONGEST_NUMBER);
    let stroke = Stroke {
        radius: f32::from_bits(1),
        hardness: f32::from_bits(1),
        color: [f32::from_bits(1); 4],
        points: vec![[tiny, tiny]; ops::MAX_POINTS],
    };
    let spec = ops::find("paint").expect("paint");
    let mut params = Params::defaults(spec);
    params.strokes_mut(1).expect("strokes").push(stroke);
    let r = Recipe {
        size: Size::new(8, 8).expect("size"),
        gloss: 1,
        blocks: vec![Block {
            x: 0,
            y: 0,
            w: 1,
            spec,
            params,
        }],
    };
    let text = write(&r);
    let longest = text.lines().map(str::len).max().unwrap_or(0);
    assert!(longest > 64 * 1024, "{longest} bytes: this case is real");
    assert!(longest <= MAX_LINE, "{longest} bytes");
    assert_eq!(read(text.as_bytes()).expect("reads back"), r);
    assert!(text.len() < MAX_FILE, "the file limit stays out of the way");
}

#[test]
fn a_stroke_over_the_point_or_stroke_cap_is_an_error_not_a_panic() {
    let tiny = longest_coordinate();
    let point = format!(" {tiny},{tiny}");
    let points = point.repeat(ops::MAX_POINTS);
    assert!(read(painted(&format!("stroke 0.1 1 1,1,1,1{points}")).as_bytes()).is_ok());
    let one_more = format!("stroke 0.1 1 1,1,1,1{points}{point}");
    assert!(read(painted(&one_more).as_bytes()).is_err());
    let line = "stroke 0.1 1 1,1,1,1 0.5,0.5\n";
    assert!(read(painted(&line.repeat(ops::MAX_STROKES)).as_bytes()).is_ok());
    assert!(read(painted(&line.repeat(ops::MAX_STROKES + 1)).as_bytes()).is_err());
}

#[test]
fn a_line_past_the_limit_is_an_error() {
    let line = format!("# {}", "x".repeat(MAX_LINE));
    assert!(read(painted(&line).as_bytes()).is_err());
}

/// The reader and the kernel apply one rule, so what reads also runs, and neither
/// moves with the size of the tile.
#[test]
fn what_the_reader_takes_of_many_strokes_paint_runs_and_no_more() {
    let thick = "stroke 0.5 1 1,1,1,1 0.5,0.5\n";
    let fits = |count: usize| read(painted(&thick.repeat(count)).as_bytes());
    let mut most = 0;
    while fits(most + 256).is_ok() && most + 256 <= ops::MAX_STROKES {
        most += 256;
    }
    assert!((256..ops::MAX_STROKES).contains(&most), "{most} big dabs");
    let r = fits(most).expect("reads");
    let g = crate::graph::compile(&r).expect("compiles");
    let tiny = Size::new(4, 4).expect("size");
    assert!(crate::graph::run(&r, &g, tiny, Default::default()).is_ok());
    assert!(fits(most + 256).is_err());
}

/// One block of many strokes. Copying the block's strokes per line, as this once
/// did, took 12.6 s in the release profile on this file, and more in a test build.
#[test]
fn reading_many_strokes_takes_linear_work() {
    let point = " 0.5,0.5".repeat(120);
    let line = format!("stroke 0 1 1,1,1,1{point}\n");
    let text = painted(&line.repeat(ops::MAX_STROKES));
    assert!(text.len() > 3_900_000, "{} bytes", text.len());
    let start = std::time::Instant::now();
    assert!(read(text.as_bytes()).is_ok());
    let spent = start.elapsed();
    assert!(spent.as_secs() < 4, "{spent:?}");
}

/// Adding a stroke leaves the block's list where it was, so no line copies it.
#[test]
fn adding_a_stroke_does_not_copy_the_list() {
    let mut r = read(painted("stroke 0 1 1,1,1,1 0.5,0.5").as_bytes()).expect("reads");
    let block = &mut r.blocks[0];
    let held = |b: &Block| match &b.params.values()[1] {
        Value::Strokes(list) => std::sync::Arc::as_ptr(list),
        _ => panic!("a strokes value"),
    };
    let before = held(block);
    let stroke = Stroke {
        radius: 0.0,
        hardness: 1.0,
        color: [1.0; 4],
        points: Vec::new(),
    };
    add_stroke(block, stroke, 1).expect("adds");
    assert_eq!(held(block), before);
}
