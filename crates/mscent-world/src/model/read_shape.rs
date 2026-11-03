//! Reading the shape on a line of a blueprint. A shape is proved here by making its
//! brush or primitive once, so that a file whose shapes make nothing never loads.

use mscent_math::Vec3d;

use super::shape::MIN_SIDES;
use super::{Hard, MAX_MODEL_COORD, MAX_NGON_SIDES, MAX_PLATE_POINTS, Soft};
use crate::brush::Op;

type Parsed<T> = Result<T, String>;

/// A finite number no larger than a blueprint may hold.
pub(super) fn number(s: &str) -> Parsed<f64> {
    match s.parse::<f64>() {
        Ok(n) if n.is_finite() && n.abs() <= MAX_MODEL_COORD => Ok(n),
        _ => Err(format!(
            "'{s}' is not a number up to {MAX_MODEL_COORD} in size"
        )),
    }
}

pub(super) fn numbers(words: &[&str]) -> Parsed<Vec<f64>> {
    words.iter().map(|w| number(w)).collect()
}

pub(super) fn positive(n: f64) -> Parsed<f64> {
    if n > 0.0 {
        Ok(n)
    } else {
        Err(format!("{n} is not above zero"))
    }
}

fn at(n: &[f64], i: usize) -> Vec3d {
    Vec3d::new(n[i], n[i + 1], n[i + 2])
}

/// A hard shape from its words, proved by making its brush once.
pub(super) fn hard(words: &[&str]) -> Parsed<Hard> {
    let (kind, rest) = words.split_first().ok_or("this piece needs a shape")?;
    let n = numbers(rest)?;
    let shape = match *kind {
        "cube" if n.len() == 6 => Hard::Cube {
            centre: at(&n, 0),
            half: at(&n, 3),
        },
        "ngon" if n.len() == 9 => Hard::Ngon {
            a: at(&n, 0),
            b: at(&n, 3),
            ra: positive(n[6])?,
            rb: positive(n[7])?,
            sides: sides(n[8])?,
        },
        "plate" => plate(&n)?,
        _ => return Err(format!("'{kind}' with these numbers is not a hard shape")),
    };
    shape
        .brush(Op::Add, 0)
        .map_err(|e| format!("this shape makes no solid: {e:?}"))?;
    Ok(shape)
}

fn sides(n: f64) -> Parsed<u8> {
    let whole = n.fract() == 0.0;
    if whole && n >= MIN_SIDES as f64 && n <= f64::from(MAX_NGON_SIDES) {
        Ok(n as u8)
    } else {
        Err(format!("a prism has 3 to {MAX_NGON_SIDES} sides"))
    }
}

fn plate(n: &[f64]) -> Parsed<Hard> {
    let (&thick, coords) = n.split_first().ok_or("a plate needs a thickness")?;
    let count = coords.len() / 3;
    let good = coords.len() % 3 == 0 && (MIN_SIDES..=MAX_PLATE_POINTS).contains(&count);
    if !good {
        return Err(format!(
            "a plate is a thickness and 3 to {MAX_PLATE_POINTS} points"
        ));
    }
    let mut points = [Vec3d::ZERO; MAX_PLATE_POINTS];
    for (i, p) in points.iter_mut().take(count).enumerate() {
        *p = at(coords, i * 3);
    }
    Ok(Hard::Plate {
        thick: positive(thick)?,
        count: count as u8,
        points,
    })
}

/// A soft shape from its words, proved by making its primitive once.
pub(super) fn soft(words: &[&str]) -> Parsed<Soft> {
    let (kind, rest) = words.split_first().ok_or("this piece needs a shape")?;
    let n = numbers(rest)?;
    let shape = match (*kind, n.len()) {
        ("sphere", 4) => Soft::Sphere {
            centre: at(&n, 0),
            radius: n[3],
        },
        ("round_cone", 8) => Soft::RoundCone {
            a: at(&n, 0),
            b: at(&n, 3),
            ra: n[6],
            rb: n[7],
        },
        ("cuboid", 6) => Soft::Cuboid {
            min: at(&n, 0),
            max: at(&n, 3),
        },
        ("spheroid", 8) => Soft::Spheroid {
            centre: at(&n, 0),
            axis: at(&n, 3),
            along: n[6],
            across: n[7],
        },
        ("hex_prism", 11) => Soft::HexPrism {
            centre: at(&n, 0),
            axis: at(&n, 3),
            side: at(&n, 6),
            apothem: n[9],
            half_depth: n[10],
        },
        _ => return Err(format!("'{kind}' with these numbers is not a soft shape")),
    };
    if shape.prim().is_none() {
        return Err(format!("this {kind} is out of range or has no size"));
    }
    Ok(shape)
}
