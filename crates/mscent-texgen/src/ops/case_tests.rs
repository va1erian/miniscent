//! Every operator on parameters that show what it does: real mortar, strokes over
//! the edges, levels that change something, a quarter turn. At defaults several
//! operators give back their input or a flat colour, so tiling and golden bytes
//! tested there prove nothing about them. An operator missing from [`case`] fails
//! these tests, so a new one cannot slip through.

use std::sync::Arc;

use super::test_util::image_of;
use super::{Frame, Image, Params, Role, Stroke, Value, all, eval};
use crate::image::{Px, Size};
use crate::noise::{Fold, Noise};

/// How far a moved window may differ, in 16-bit levels: rounding only. One level
/// of an 8-bit texture is 257.
const MOVED_WINDOW_LEVELS: i32 = 96;

/// Wide enough for fine parameters to show, and not square, so a kernel that
/// mixes up its sides shows too.
fn size() -> Size {
    Size::new(256, 128).expect("size")
}

fn i(v: i32) -> Value {
    Value::Int(v)
}

fn f(v: f32) -> Value {
    Value::Float(v)
}

/// Strokes over the right edge, the bottom edge and a corner, thick and thin, soft
/// and hard, and a lone dab, in tile coordinates past the tile on purpose.
fn strokes_over_the_edges() -> Value {
    let stroke = |radius, hardness, color, points: &[[f32; 2]]| Stroke {
        radius,
        hardness,
        color,
        points: points.to_vec(),
    };
    Value::Strokes(Arc::new(vec![
        stroke(
            0.06,
            0.3,
            [1.0, 0.4, 0.2, 1.0],
            &[[0.8, 0.3], [1.0, 0.4], [1.3, 0.45]],
        ),
        stroke(0.02, 1.0, [1.0; 4], &[[0.5, 0.8], [0.55, 1.1], [0.6, 1.4]]),
        stroke(
            0.03,
            0.8,
            [0.2, 0.6, 1.0, 0.5],
            &[[0.9, 0.9], [1.05, 1.05], [1.2, 1.2]],
        ),
        stroke(0.05, 0.5, [0.8; 4], &[[-0.1, -0.1]]),
    ]))
}

/// Parameters of an operator at which it does something. `None` for an operator
/// nobody has given a case yet, which fails the tests.
fn case(name: &str) -> Option<Vec<(&'static str, Value)>> {
    Some(match name {
        "flat" => vec![("color", Value::Color([0.25, 0.5, 0.75, 1.0]))],
        "noise" => vec![
            ("freq_x", i(3)),
            ("freq_y", i(5)),
            ("octaves", i(5)),
            ("seed", i(7)),
            ("mode", Value::Choice(1)),
        ],
        "checker" => vec![("cols", i(6)), ("rows", i(4))],
        "stripes" => vec![("cycles_x", i(3)), ("cycles_y", i(-2)), ("duty", f(0.3))],
        "gradient" => vec![("cycles_x", i(2)), ("cycles_y", i(1))],
        "glow" => vec![
            ("x", f(0.9)),
            ("y", f(0.95)),
            ("radius", f(0.4)),
            ("power", f(2.5)),
        ],
        "bricks" => vec![
            ("rows", i(4)),
            ("cols", i(3)),
            ("offset", f(0.5)),
            ("mortar", f(0.2)),
            ("bevel", f(0.3)),
            ("mode", Value::Choice(2)),
        ],
        "cells" => vec![("count", i(9)), ("seed", i(4)), ("mode", Value::Choice(1))],
        "wavelet" => vec![("freq", i(3)), ("octaves", i(4)), ("seed", i(2))],
        "rect" => vec![
            ("x", f(0.95)),
            ("y", f(0.1)),
            ("w", f(0.4)),
            ("h", f(0.5)),
            ("softness", f(0.05)),
        ],
        "ellipse" => vec![
            ("x", f(0.05)),
            ("y", f(0.9)),
            ("w", f(0.6)),
            ("h", f(0.3)),
            ("softness", f(0.02)),
        ],
        "line" => vec![
            ("x0", f(0.9)),
            ("y0", f(0.1)),
            ("x1", f(0.2)),
            ("y1", f(0.95)),
            ("width", f(0.06)),
        ],
        "dots" => vec![
            ("count_x", i(5)),
            ("count_y", i(3)),
            ("radius", f(0.3)),
            ("softness", f(0.1)),
            ("jitter", f(0.8)),
            ("seed", i(9)),
        ],
        "paint" => vec![("strokes", strokes_over_the_edges())],
        "scratches" => vec![
            ("count", i(24)),
            ("length", f(0.4)),
            ("width", f(0.02)),
            ("seed", i(3)),
        ],
        "text" => vec![
            ("text", Value::Text("TILE".to_string())),
            ("x", f(0.8)),
            ("y", f(0.9)),
            ("scale", f(0.04)),
        ],
        "merge" => vec![("mode", Value::Choice(6))],
        "mask" => Vec::new(),
        "paste" => vec![("x", f(0.9)), ("y", f(0.9)), ("w", f(0.5)), ("h", f(0.25))],
        "blur" => vec![("radius", f(0.05))],
        "color" => vec![
            ("gain", Value::Color([0.5, 1.0, 0.75, 0.9])),
            ("amount", f(1.5)),
        ],
        "range" => vec![
            ("in_low", f(0.2)),
            ("in_high", f(0.8)),
            ("out_low", f(0.1)),
            ("out_high", f(0.9)),
            ("gamma", f(1.8)),
        ],
        "invert" => Vec::new(),
        "hscb" => vec![
            ("hue", f(0.3)),
            ("saturation", f(1.5)),
            ("contrast", f(1.25)),
            ("brightness", f(0.8)),
        ],
        "sharpen" => vec![("amount", f(2.0)), ("radius", f(0.03))],
        "rotozoom" => vec![
            ("turns", i(1)),
            ("zoom_x", i(2)),
            ("zoom_y", i(3)),
            ("shift_x", f(0.25)),
            ("shift_y", f(0.125)),
        ],
        "twirl" => vec![("strength", f(4.0)), ("radius", f(0.45))],
        "bulge" => vec![("strength", f(0.7)), ("radius", f(0.45))],
        "distort" => vec![("amount", f(0.2))],
        "bump" => vec![
            ("light_x", f(0.6)),
            ("light_y", f(-0.4)),
            ("strength", f(12.0)),
            ("ambient", f(0.25)),
        ],
        "cavity" => vec![("radius", f(0.1)), ("amount", f(6.0))],
        "slope" => vec![("strength", f(12.0))],
        // These only pass their input on: the graph never runs their kernels, and
        // what they must do is give it back.
        "store" | "out-albedo" | "out-specular" | "out-alpha" => Vec::new(),
        "out-height" => vec![("normal_strength", f(8.0))],
        _ => return None,
    })
}

/// A varied RGBA input that tiles, different per seed.
fn sample(seed: u32) -> Image {
    let n = |s| Noise {
        freq: (5, 3),
        octaves: 4,
        persistence: 0.6,
        seed: seed.wrapping_mul(7).wrapping_add(s),
        fold: Fold::Fbm,
    };
    let [r, g, b, a] = [n(0), n(1), n(2), n(3)];
    let frame = Frame::new(size());
    image_of(size(), |x, y| {
        let (u, v) = frame.uv(x, y);
        [
            r.at(u, v) * 0.5 + 0.5,
            g.at(u, v) * 0.5 + 0.5,
            b.at(u, v) * 0.5 + 0.5,
            a.at(u, v) * 0.5 + 0.5,
        ]
    })
}

fn moved_half(image: &Image) -> Image {
    let s = image.size();
    let (dx, dy) = (s.width() as i32 / 2, s.height() as i32 / 2);
    Image::from_xy(s, |x, y| image.at(x + dx, y + dy))
}

fn worst_difference(a: &Image, b: &Image) -> i32 {
    let channel = |p: Px, q: Px| {
        (0..4)
            .map(|c| (i32::from(p[c]) - i32::from(q[c])).abs())
            .max()
            .unwrap_or(0)
    };
    a.pixels()
        .iter()
        .zip(b.pixels())
        .map(|(&p, &q)| channel(p, q))
        .max()
        .unwrap_or(0)
}

struct Case {
    spec: &'static super::OpSpec,
    params: Params,
    inputs: Vec<Image>,
}

impl Case {
    fn run(&self, inputs: &[Image], origin: [f32; 2]) -> Image {
        let refs: Vec<&Image> = inputs.iter().collect();
        let frame = Frame {
            size: size(),
            origin,
        };
        eval(self.spec, &self.params, &refs, frame).expect(self.spec.name)
    }
}

/// The case of each operator that has a kernel to run, with its inputs.
fn cases() -> impl Iterator<Item = Case> {
    all().filter(|op| op.role != Role::Load).map(|spec| {
        let set = case(spec.name).unwrap_or_else(|| panic!("{} has no case", spec.name));
        let mut params = Params::defaults(spec);
        for (key, value) in set {
            params.set(spec, key, value).expect(key);
        }
        let count = spec.inputs.min.max(spec.inputs.max.min(1));
        let inputs = (0..u32::from(count)).map(sample).collect();
        Case {
            spec,
            params,
            inputs,
        }
    })
}

/// A pattern that does not repeat, or a filter that does not wrap, draws a
/// different picture when its window moves by half a tile.
#[test]
fn every_operator_tiles() {
    for c in cases() {
        let here = c.run(&c.inputs, [0.0, 0.0]);
        let moved: Vec<Image> = c.inputs.iter().map(moved_half).collect();
        let there = c.run(&moved, [0.5, 0.5]);
        let worst = worst_difference(&moved_half(&here), &there);
        assert!(
            worst <= MOVED_WINDOW_LEVELS,
            "{}: off by {worst}",
            c.spec.name
        );
    }
}

/// The tiling test is worth nothing on a flat picture or on the input coming back.
#[test]
fn every_case_shows_something() {
    for c in cases() {
        let name = c.spec.name;
        let image = c.run(&c.inputs, [0.0, 0.0]);
        let first = image.pixels().first().copied();
        if matches!(c.spec.role, Role::Store | Role::Output(_)) {
            assert_eq!(Some(&image), c.inputs.first(), "{name} passes its input on");
        } else if name == "flat" {
            assert_ne!(first, Some([65535; 4]), "not the default colour");
        } else {
            assert!(image.pixels().iter().any(|p| Some(*p) != first), "{name}");
            assert_ne!(Some(&image), c.inputs.first(), "{name} changed nothing");
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

/// One set of bytes for one recipe, on parameters that show it. Change a value here
/// only for a kernel you meant to change.
const GOLDEN: &[(&str, u64)] = &[
    ("flat", 0xf6c3af5282b72325),
    ("noise", 0x9c6b139d31776590),
    ("checker", 0x590ef3967a8a3b25),
    ("stripes", 0x24f469f827f13425),
    ("gradient", 0x4fa33712fc061325),
    ("glow", 0x6de30582e23410f3),
    ("bricks", 0xb8ba37febc218165),
    ("cells", 0xea01724e8689d4c),
    ("wavelet", 0x8ad82dea886f17e8),
    ("rect", 0xa56519b3cd80a52c),
    ("ellipse", 0x15be67601aa1e45b),
    ("line", 0x3340c11be4eb383),
    ("dots", 0x1b9843022898baa2),
    ("paint", 0xbc7f6acd783f7cdb),
    ("scratches", 0x6cea30006866ad35),
    ("text", 0xd94bb03bf8565026),
    ("merge", 0x351e495aa3ed1838),
    ("mask", 0xfe4eb676b3c7c7a4),
    ("paste", 0x367648fe12cb99f5),
    ("blur", 0xfa340c2aa6c0c2f),
    ("color", 0x146ab53e712d25e7),
    ("range", 0x53a010a6d99021fe),
    ("invert", 0x109c483a254ea476),
    ("hscb", 0xeda6c25eca9a9cf3),
    ("sharpen", 0x311de8f6019e6f72),
    ("rotozoom", 0x403e092492cd337d),
    ("twirl", 0xbd4a0ee0e8841684),
    ("bulge", 0x24b2dd6e40b1dc31),
    ("distort", 0x3b33b1b8a04480af),
    ("bump", 0x78f08c990afeb9b2),
    ("cavity", 0x71aea13d97fa73ef),
    ("slope", 0xb0ef32c28b18e8f9),
    ("store", 0xb2aa8fbdd9e99f96),
    ("out-albedo", 0xb2aa8fbdd9e99f96),
    ("out-height", 0xb2aa8fbdd9e99f96),
    ("out-specular", 0xb2aa8fbdd9e99f96),
    ("out-alpha", 0xb2aa8fbdd9e99f96),
];

#[test]
fn every_case_makes_its_golden_bytes() {
    let mut wrong = Vec::new();
    for c in cases() {
        let hash = fnv(&c.run(&c.inputs, [0.0, 0.0]));
        let golden = GOLDEN.iter().find(|(name, _)| *name == c.spec.name);
        if golden.map(|g| g.1) != Some(hash) {
            wrong.push(format!("(\"{}\", {hash:#x}),", c.spec.name));
        }
    }
    assert!(
        wrong.is_empty(),
        "golden table should read:\n{}",
        wrong.join("\n")
    );
}
