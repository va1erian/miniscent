//! Edits to a volume. An edit reads and writes `edited` values and stores them
//! as changes over the base, so the brushes below stay free to move.

use mscent_math::Vec3d;

use super::context::Context;
use super::noise::noise3;
use super::prim::Prim;
use super::span::{Reader, Span, collect_edited, edit_samples};
use super::volume::Volume;
use super::{MAX_SAMPLE, quantize};

/// Cells past a shape where an add or carve can still change a sample: the
/// sample range is two cells deep.
const BAND_CELLS: f64 = 2.0;
/// How fast a smoothing dab reaches full strength going in from its edge,
/// against the diagonal of its bounds.
const CORE_SHARPNESS: f64 = 4.0;
/// Cells between the bumps a roughening dab makes.
const BUMP_CELLS: f64 = 3.0;

/// What a dab does inside its shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DabOp {
    /// Fills with solid of a material.
    Add { material: u8 },
    /// Removes solid.
    Carve,
    /// Moves samples toward the average of their neighbours, fully at the
    /// shape's core and not at all at its edge.
    Smooth,
    /// Changes the material only.
    Paint { material: u8 },
    /// Moves samples toward the plane through `origin` whose unit `normal`
    /// points out of the solid, fully at the shape's core. Solid it adds is
    /// of `material`.
    Flatten {
        material: u8,
        origin: Vec3d,
        normal: Vec3d,
    },
    /// Displaces the surface by seeded noise of up to `amplitude` samples (1/32
    /// of a cell), fully at the shape's core. The same seed bumps the same
    /// places, so repeated dabs deepen the bumps rather than adding new ones.
    /// Solid it adds is of `material`.
    Roughen {
        seed: u32,
        amplitude: u8,
        material: u8,
    },
}

/// Applies one dab. A context built for another volume, or a shape that misses
/// the box, changes nothing. Returns the chunks to remesh.
pub fn apply_dab(volume: &mut Volume, ctx: &mut Context, prim: &Prim, op: DabOp) -> Vec<[u16; 3]> {
    if ctx.lattice != volume.lattice {
        return Vec::new();
    }
    let lattice = volume.lattice;
    let cell = lattice.cell.units();
    let (lo, hi) = prim.bounds();
    let band = match op {
        DabOp::Add { .. } | DabOp::Carve => BAND_CELLS,
        DabOp::Smooth | DabOp::Paint { .. } | DabOp::Flatten { .. } | DabOp::Roughen { .. } => 0.0,
    };
    let Some(span) = Span::around(&lattice, lo, hi, band) else {
        return Vec::new();
    };
    match op {
        DabOp::Add { material } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            let shape = quantize(-prim.distance(p), cell);
            let m = if shape >= 0 { material } else { old };
            (value.max(shape), m)
        }),
        DabOp::Carve => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            (value.min(quantize(prim.distance(p), cell)), old)
        }),
        DabOp::Paint { material } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            (
                value,
                if prim.distance(p) <= 0.0 {
                    material
                } else {
                    old
                },
            )
        }),
        DabOp::Smooth => smooth(volume, ctx, prim, span),
        DabOp::Flatten {
            material,
            origin,
            normal,
        } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            let weight = core_weight(prim, p, cell);
            let plane = quantize(-normal.dot(p - origin), cell);
            let new = blend(value, plane, weight);
            (new, grown_material(value, new, material, old))
        }),
        DabOp::Roughen {
            seed,
            amplitude,
            material,
        } => edit_samples(volume, ctx, span, &mut |_, p, value, old| {
            if value.abs() == MAX_SAMPLE {
                return (value, old);
            }
            let bump = noise3(seed, p * (1.0 / (BUMP_CELLS * cell))) * f64::from(amplitude);
            let moved = f64::from(value) + bump * core_weight(prim, p, cell);
            let new = moved
                .round()
                .clamp(-f64::from(MAX_SAMPLE), f64::from(MAX_SAMPLE)) as i8;
            (new, grown_material(value, new, material, old))
        }),
    }
}

/// How far into a shape a point is, from 0 at its edge to 1 at its core.
fn core_weight(prim: &Prim, p: Vec3d, cell: f64) -> f64 {
    let (lo, hi) = prim.bounds();
    let size = (hi - lo).length().max(cell);
    (-prim.distance(p) / size * CORE_SHARPNESS).clamp(0.0, 1.0)
}

/// `value` moved `weight` of the way to `target`.
fn blend(value: i8, target: i8, weight: f64) -> i8 {
    let moved = f64::from(value) + (f64::from(target) - f64::from(value)) * weight;
    moved
        .round()
        .clamp(-f64::from(MAX_SAMPLE), f64::from(MAX_SAMPLE)) as i8
}

/// An air sample's material is whatever it last held, so solid it grows into
/// takes the dab's.
fn grown_material(before: i8, after: i8, material: u8, old: u8) -> u8 {
    if before < 0 && after >= 0 {
        material
    } else {
        old
    }
}

/// Smoothing reads all values first, so the result does not depend on the
/// order samples are visited in.
fn smooth(volume: &mut Volume, ctx: &mut Context, prim: &Prim, span: Span) -> Vec<[u16; 3]> {
    let cell = volume.lattice.cell.units();
    let mut values = Vec::new();
    collect_edited(volume, ctx, span, &mut values);
    let reader = Reader {
        span,
        values: &values,
    };
    edit_samples(volume, ctx, span, &mut |g, p, value, old| {
        let new = blend(value, reader.average(g) as i8, core_weight(prim, p, cell));
        // An air sample's material is whatever it last held, never seen until now.
        let grew = value < 0 && new >= 0;
        let material = grew.then(|| reader.solid_material(g)).flatten();
        (new, material.unwrap_or(old))
    })
}

/// Replaces every sample from a field in world units, positive solid: the Fill
/// tool, for interiors that start from a formula. Returns the chunks to remesh.
pub fn fill(
    volume: &mut Volume,
    ctx: &mut Context,
    field: impl Fn(Vec3d) -> f64,
    material: u8,
) -> Vec<[u16; 3]> {
    if ctx.lattice != volume.lattice {
        return Vec::new();
    }
    let span = Span {
        first: [0; 3],
        end: volume.lattice.samples(),
    };
    let cell = volume.lattice.cell.units();
    edit_samples(volume, ctx, span, &mut |_, p, _, _| {
        (quantize(field(p), cell), material)
    })
}
