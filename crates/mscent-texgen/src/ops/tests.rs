//! Tests every operator gets from the table: determinism against golden hashes,
//! and survival at the ends of every parameter's range. Tiling is `case_tests.rs`,
//! with parameters that show something. An operator's own behaviour is tested
//! beside its kernel.

use super::*;
use crate::image::Size;
use crate::noise::{Fold, Noise};

fn size() -> Size {
    Size::new(64, 32).expect("size")
}

/// A varied RGBA input that tiles, different per seed.
fn sample(seed: u32) -> Image {
    let n = |s| Noise {
        freq: (4, 2),
        octaves: 3,
        persistence: 0.5,
        seed: seed.wrapping_mul(7).wrapping_add(s),
        fold: Fold::Fbm,
    };
    let [r, g, b, a] = [n(0), n(1), n(2), n(3)];
    Frame::new(size()).fill(|u, v| {
        [
            r.at(u, v) * 0.5 + 0.5,
            g.at(u, v) * 0.5 + 0.5,
            b.at(u, v) * 0.5 + 0.5,
            a.at(u, v) * 0.5 + 0.5,
        ]
    })
}

fn inputs_for(spec: &OpSpec) -> Vec<Image> {
    (0..u32::from(spec.inputs.min).max(u32::from(spec.inputs.max.min(1))))
        .map(sample)
        .collect()
}

fn runnable() -> impl Iterator<Item = &'static OpSpec> {
    all().filter(|op| op.role != Role::Load)
}

#[test]
fn names_are_unique_and_found() {
    let mut names: Vec<_> = all().map(|op| op.name).collect();
    let count = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count);
    assert!(all().all(|op| find(op.name) == Some(op)));
    assert!(find("no such op").is_none());
}

#[test]
fn every_default_is_in_range() {
    for op in all() {
        for p in op.params {
            let value = p.kind.default_value();
            assert!(p.kind.accepts(&value), "{}.{}", op.name, p.name);
        }
    }
}

fn fnv(image: &Image) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for p in image.pixels() {
        for c in p {
            h = (h ^ u64::from(*c)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// One set of bytes for one recipe, on every platform CI and development use, at
/// each operator's defaults; `case_tests.rs` has the same for parameters that show
/// more. Change a value here only for a kernel you meant to change.
const GOLDEN: &[(&str, u64)] = &[
    ("flat", 0x4c342288a5868325),
    ("noise", 0xbb8f2cabeccfa65a),
    ("merge", 0x56bea892146a755b),
    ("blur", 0xa2f5e8b2aea4a150),
    ("checker", 0x57436a753d2ff325),
    ("stripes", 0x47e16dc6fcb19325),
    ("gradient", 0x655b649412af325),
    ("glow", 0xd360261768f20e35),
    ("bricks", 0x4c342288a5868325),
    ("cells", 0x60adf7a4c2e7f539),
    ("wavelet", 0xbd9338baafa89f9f),
    ("rect", 0x59c21aac75bbdc7a),
    ("ellipse", 0x8cdc9812c462516a),
    ("line", 0x32ab38ac0e400e1a),
    ("dots", 0x75713549cdb872b4),
    ("paint", 0x30fa03c261f1b0d2),
    ("scratches", 0xf2d1aa6d71d4d0eb),
    ("text", 0xb4426b723733b36d),
    ("mask", 0x4f57f79696ab8d9),
    ("paste", 0xa64931ec18e147a8),
    ("color", 0x30fa03c261f1b0d2),
    ("range", 0x30fa03c261f1b0d2),
    ("invert", 0xa5961ff601c0210a),
    ("hscb", 0x30fa03c261f1b0d2),
    ("sharpen", 0xbcc3b18f674a831f),
    ("rotozoom", 0x30fa03c261f1b0d2),
    ("twirl", 0x4113aedcd33a18f9),
    ("bulge", 0xea4946ce3903152a),
    ("distort", 0x60b12062ccdc4e1a),
    ("bump", 0x76096a84e34df713),
    ("cavity", 0xd1d25559a7bdb9eb),
    ("slope", 0xa88be295e41ba737),
    ("store", 0x30fa03c261f1b0d2),
    ("out-albedo", 0x30fa03c261f1b0d2),
    ("out-height", 0x30fa03c261f1b0d2),
    ("out-specular", 0x30fa03c261f1b0d2),
    ("out-alpha", 0x30fa03c261f1b0d2),
];

#[test]
fn every_operator_makes_its_golden_bytes() {
    let mut wrong = Vec::new();
    for op in runnable() {
        let inputs = inputs_for(op);
        let refs: Vec<&Image> = inputs.iter().collect();
        let image = eval(op, &Params::defaults(op), &refs, Frame::new(size())).expect(op.name);
        let hash = fnv(&image);
        let golden = GOLDEN.iter().find(|(name, _)| *name == op.name);
        if golden.map(|g| g.1) != Some(hash) {
            wrong.push(format!("(\"{}\", {hash:#x}),", op.name));
        }
    }
    assert!(
        wrong.is_empty(),
        "golden table should read:\n{}",
        wrong.join("\n")
    );
}

/// The ends of each range, one at a time and all together. Errors are fine (a
/// count that cannot tile is one); a panic, or a wrong size, is not.
#[test]
fn every_operator_survives_its_extremes() {
    for op in runnable() {
        let inputs = inputs_for(op);
        let refs: Vec<&Image> = inputs.iter().collect();
        let check = |params: &Params| {
            if let Ok(image) = eval(op, params, &refs, Frame::new(size())) {
                assert_eq!(image.size(), size(), "{}", op.name);
            }
        };
        for end in 0..2 {
            let mut all_ends = Params::defaults(op);
            for p in op.params {
                let value = match (p.kind, end) {
                    (ParamKind::Float { min, .. }, 0) => Value::Float(min),
                    (ParamKind::Float { max, .. }, _) => Value::Float(max),
                    (ParamKind::Int { min, .. }, 0) => Value::Int(min),
                    (ParamKind::Int { max, .. }, _) => Value::Int(max),
                    (ParamKind::Choice { .. }, 0) => Value::Choice(0),
                    (ParamKind::Choice { names, .. }, _) => Value::Choice(names.len() as u8 - 1),
                    (ParamKind::Color { .. }, 0) => Value::Color([0.0; 4]),
                    (ParamKind::Color { .. }, _) => Value::Color([1.0; 4]),
                    _ => continue,
                };
                let mut one = Params::defaults(op);
                one.set(op, p.name, value.clone()).expect(p.name);
                check(&one);
                all_ends.set(op, p.name, value).expect(p.name);
            }
            check(&all_ends);
        }
    }
}

#[test]
fn eval_rejects_inputs_that_do_not_fit() {
    let (blur, other) = (find("blur").expect("blur"), sample(0));
    let params = Params::defaults(blur);
    let frame = Frame::new(size());
    assert!(eval(blur, &params, &[], frame).is_err());
    assert!(eval(blur, &params, &[&other, &other], frame).is_err());
    let small = Image::flat(Size::new(8, 8).expect("size"), [0; 4]);
    assert!(eval(blur, &params, &[&small], frame).is_err());
}

#[test]
fn set_rejects_what_the_table_does_not_allow() {
    let noise = find("noise").expect("noise");
    let mut p = Params::defaults(noise);
    assert!(p.set(noise, "octaves", Value::Int(9)).is_err());
    assert!(p.set(noise, "octaves", Value::Float(2.0)).is_err());
    assert!(p.set(noise, "persistence", Value::Float(f32::NAN)).is_err());
    assert!(p.set(noise, "mode", Value::Choice(2)).is_err());
    assert!(p.set(noise, "nope", Value::Int(1)).is_err());
    assert_eq!(p, Params::defaults(noise), "failed sets change nothing");
    p.set(noise, "octaves", Value::Int(8)).expect("in range");
    assert_ne!(p, Params::defaults(noise));
}

#[test]
fn a_flat_noise_reads_as_its_first_colour_not_a_division_by_zero() {
    let noise = find("noise").expect("noise");
    let mut p = Params::defaults(noise);
    p.set(noise, "freq_x", Value::Int(0)).expect("x");
    p.set(noise, "freq_y", Value::Int(0)).expect("y");
    let image = eval(noise, &p, &[], Frame::new(size())).expect("noise");
    assert!(image.pixels().iter().all(|&q| q == [0, 0, 0, 65535]));
}

#[test]
fn noise_fills_the_range() {
    let noise = find("noise").expect("noise");
    let image = eval(noise, &Params::defaults(noise), &[], Frame::new(size())).expect("noise");
    let reds = || image.pixels().iter().map(|p| p[0]);
    assert_eq!(reds().min(), Some(0));
    assert_eq!(reds().max(), Some(65535));
}

#[test]
fn a_blur_keeps_the_mean_and_smooths() {
    let blur = find("blur").expect("blur");
    let source = sample(3);
    let mut p = Params::defaults(blur);
    p.set(blur, "radius", Value::Float(0.05)).expect("radius");
    let out = eval(blur, &p, &[&source], Frame::new(size())).expect("blur");
    let mean = |i: &Image| {
        i.pixels().iter().map(|q| f64::from(q[0])).sum::<f64>() / i.pixels().len() as f64
    };
    assert!((mean(&source) - mean(&out)).abs() < 8.0);
    let step = |i: &Image| {
        (0..64)
            .map(|x| (i32::from(i.at(x, 3)[0]) - i32::from(i.at(x + 1, 3)[0])).abs())
            .max()
    };
    assert!(step(&out) < step(&source));
}

#[test]
fn a_blur_of_radius_zero_changes_nothing() {
    let blur = find("blur").expect("blur");
    let source = sample(4);
    let mut p = Params::defaults(blur);
    p.set(blur, "radius", Value::Float(0.0)).expect("radius");
    assert_eq!(eval(blur, &p, &[&source], Frame::new(size())), Ok(source));
}

#[test]
fn merge_modes_do_what_they_say() {
    let merge = find("merge").expect("merge");
    let (a, b) = (
        Image::flat(size(), [32768, 32768, 32768, 65535]),
        Image::flat(size(), [16384, 0, 65535, 32768]),
    );
    let run = |mode: u8| {
        let mut p = Params::defaults(merge);
        p.set(merge, "mode", Value::Choice(mode)).expect("mode");
        eval(merge, &p, &[&a, &b], Frame::new(size()))
            .expect("merge")
            .at(0, 0)
    };
    assert_eq!(run(0)[0], 49152, "add");
    assert_eq!(run(1)[0], 16384, "subtract");
    assert_eq!(run(2)[2], 32768, "multiply by one");
    assert_eq!(run(3)[0], 16384, "min");
    assert_eq!(run(4)[0], 32768, "max");
    assert_eq!(run(6)[2], 49152, "half-covering blend");
}
