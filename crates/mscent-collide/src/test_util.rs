//! Shared level builders for the collision tests.

use mscent_math::{DPlane, Vec3, Vec3d};
use mscent_world::{Brush, Bsp, Op, compile};

pub fn v(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

pub fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// Empty cube room `[-20, 20]^3` inside solid rock.
pub fn room_brush() -> Brush {
    Brush::cuboid(
        vd(-20.0, -20.0, -20.0),
        vd(20.0, 20.0, 20.0),
        Op::Subtract,
        0,
    )
    .unwrap()
}

pub fn room() -> Bsp {
    compile(&[room_brush()]).unwrap()
}

/// The room plus a solid block `[0, 4]^3` standing in it.
pub fn room_with_block() -> Bsp {
    let block = Brush::cuboid(vd(0.0, 0.0, 0.0), vd(4.0, 4.0, 4.0), Op::Add, 1).unwrap();
    compile(&[room_brush(), block]).unwrap()
}

/// The room plus a prism whose top is the 45 degree slope `z = x`
/// (solid below it), spanning `x, y` in `[-10, 10]`.
pub fn room_with_ramp() -> Bsp {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let planes = [
        DPlane::new(vd(1.0, 0.0, 0.0), 10.0),
        DPlane::new(vd(-1.0, 0.0, 0.0), 10.0),
        DPlane::new(vd(0.0, 1.0, 0.0), 10.0),
        DPlane::new(vd(0.0, -1.0, 0.0), 10.0),
        DPlane::new(vd(0.0, 0.0, -1.0), 10.0),
        DPlane::new(vd(-s, 0.0, s), 0.0),
    ];
    let ramp = Brush::new(&planes, Op::Add, 1).unwrap();
    compile(&[room_brush(), ramp]).unwrap()
}

/// Exact clearance of a sphere centre in [`room_with_block`]: distance to the
/// nearest wall or to the block, whichever is smaller. An oracle independent
/// of the BSP code under test.
pub fn block_room_clearance(p: Vec3) -> f32 {
    let walls = [
        20.0 - p.x,
        p.x + 20.0,
        20.0 - p.y,
        p.y + 20.0,
        20.0 - p.z,
        p.z + 20.0,
    ];
    let wall = walls.into_iter().fold(f32::MAX, f32::min);
    let out = |c: f32| (0.0 - c).max(c - 4.0).max(0.0);
    let block = (out(p.x).powi(2) + out(p.y).powi(2) + out(p.z).powi(2)).sqrt();
    wall.min(block)
}
