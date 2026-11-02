//! Running-bond brick walls: a mask, a grey per brick, or a bevelled height.

use super::cost::Cost;
use super::util::{grey, unit_hash};
use super::{Ctx, OpSpec, Role, choice, exactly, float, int};
use crate::error::TexError;
use crate::image::Image;
use crate::math::frac;

/// Where a texel sits in the wall.
struct Spot {
    /// Row and column, wrapped into the tile, which is what the hash reads.
    row: i32,
    col: i32,
    /// Distance to the nearest brick edge in tile units; negative in the mortar.
    depth: f32,
}

struct Wall {
    rows: i32,
    cols: i32,
    /// Shift of odd rows, in bricks.
    offset: f32,
    /// Mortar width in tile units, the same along both axes.
    mortar: f32,
}

impl Wall {
    fn spot(&self, u: f32, v: f32) -> Spot {
        let ry = v * self.rows as f32;
        let row = (ry.floor() as i64).rem_euclid(i64::from(self.rows)) as i32;
        let shift = if row & 1 == 1 { self.offset } else { 0.0 };
        let cx = u * self.cols as f32 - shift;
        let col = (cx.floor() as i64).rem_euclid(i64::from(self.cols)) as i32;
        let (fx, fy) = (frac(cx), frac(ry));
        let to_edge_x = fx.min(1.0 - fx) / self.cols as f32;
        let to_edge_y = fy.min(1.0 - fy) / self.rows as f32;
        Spot {
            row,
            col,
            depth: to_edge_x.min(to_edge_y) - self.mortar / 2.0,
        }
    }
}

fn bricks(ctx: &Ctx) -> Result<Image, TexError> {
    let (rows, cols) = (ctx.int("rows"), ctx.int("cols"));
    let offset = ctx.float("offset");
    if rows % 2 != 0 && offset != 0.0 {
        return Err(TexError::NotTiling("odd brick rows cannot alternate"));
    }
    let wall = Wall {
        rows,
        cols,
        offset,
        mortar: ctx.float("mortar") / rows as f32,
    };
    let seed = ctx.int("seed") as u32;
    // A bevel is a share of a brick's height, so it keeps its look at any count.
    let ramp = ctx.float("bevel") / rows as f32;
    let mode = ctx.choice("mode");
    Ok(ctx.frame.fill(|u, v| {
        let spot = wall.spot(u, v);
        let brick = spot.depth >= 0.0;
        grey(match mode {
            0 => f32::from(u8::from(brick)),
            1 if brick => 0.3 + 0.7 * unit_hash(spot.row, spot.col, seed),
            1 => 0.0,
            _ if ramp > 0.0 => (spot.depth / ramp).clamp(0.0, 1.0),
            _ => f32::from(u8::from(brick)),
        })
    }))
}

pub(super) const OPS: &[OpSpec] = &[OpSpec {
    name: "bricks",
    group: "Pattern",
    inputs: exactly(0),
    params: &[
        int("rows", 2, 64, 8),
        int("cols", 1, 64, 4),
        float("offset", 0.0, 1.0, 0.5),
        float("mortar", 0.0, 0.25, 0.0625),
        float("bevel", 0.0, 0.5, 0.1),
        int("seed", 0, 65535, 1),
        choice("mode", &["mask", "tint", "bevel"], 0),
    ],
    role: Role::Kernel,
    kernel: bricks,
    cost: Cost::Flat(9.0),
}];
