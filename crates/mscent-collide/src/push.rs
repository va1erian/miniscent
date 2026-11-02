//! Moving a mover through the bodies in its way (M10.3).
//!
//! Each body's position is read in the mover's frame at the old and new
//! poses. The line between them is the body's path relative to the mover, so
//! a fast mover cannot jump over a body. A body it meets is carried along,
//! then moved in the world by the ordinary slide, which keeps walls and other
//! movers in force. If that cannot clear the mover, the body is being
//! squeezed: the step is undone and the caller decides what happens.
//!
//! The path is a straight chord even when the mover turns. Its ends are exact,
//! so however fast a mover spins no body ends up inside it, and the fuzz test
//! holds at a turn of 1.5 radians per step. Only the path between is
//! approximate, which at worst lets a blade's tip pass a body without touching.

use crate::mover::{Frame, MoverSolid, Pose, frame_bounds};
use crate::slide::slide_move;
use crate::step::Body;
use crate::sweep::Solids;
use crate::tuning::{SKIN, SQUEEZE_SLACK};
use mscent_math::{Vec3, Vec3d};
use mscent_world::Bsp;

/// Rounds of pushing a wedged body out. Between a wall and a mover at an angle,
/// each round slides it part of the way; at 45 degrees it halves the overlap.
const WEDGE_ROUNDS: usize = 8;

/// What became of a mover's step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    /// The mover is at its new pose and every body in the way was moved.
    Moved,
    /// The mover stayed where it was and no body changed, because this body
    /// (an index into the slice given to [`push_mover`]) has nowhere to go.
    Blocked(usize),
}

/// Moves `movers[index]` to `pose`, pushing `bodies`. Never leaves a body
/// inside a mover or through a wall that it was clear of.
pub fn push_mover(
    bsp: &Bsp,
    movers: &mut [MoverSolid],
    index: usize,
    pose: &Pose,
    bodies: &mut [&mut Body],
) -> Push {
    let Some(mover) = movers.get(index) else {
        return Push::Moved;
    };
    let (shape, old) = (mover.shape.clone(), mover.frame);
    let new = Frame::of(pose);
    if old == new {
        return Push::Moved;
    }
    let (new_lo, new_hi) = frame_bounds(&shape, new);
    let (lo, hi) = (mover.lo.min(new_lo), mover.hi.max(new_hi));
    let mut moved: Vec<(usize, Vec3)> = Vec::new();
    for i in 0..bodies.len() {
        if !near(bodies[i], lo, hi) {
            continue;
        }
        let before = bodies[i].pos;
        let solids = Solids::new(bsp, movers).without(index);
        if !push_body(solids, &old, &new, &shape, bodies[i]) {
            for &(j, pos) in &moved {
                bodies[j].pos = pos;
            }
            return Push::Blocked(i);
        }
        if bodies[i].pos != before {
            moved.push((i, before));
        }
    }
    movers[index].set_frame(new);
    Push::Moved
}

fn near(body: &Body, lo: Vec3d, hi: Vec3d) -> bool {
    let p = body.pos.to_f64();
    let m = f64::from(body.radius) + crate::tuning::PRUNE_MARGIN;
    p.x + m >= lo.x
        && p.x - m <= hi.x
        && p.y + m >= lo.y
        && p.y - m <= hi.y
        && p.z + m >= lo.z
        && p.z - m <= hi.z
}

/// False when the body cannot get out of the way.
fn push_body(
    solids: Solids,
    old: &Frame,
    new: &Frame,
    shape: &crate::mover::MoverShape,
    body: &mut Body,
) -> bool {
    let p = body.pos.to_f64();
    let r = f64::from(body.radius);
    let (from, to) = (old.to_local(p), new.to_local(p));
    // Already inside: sweeps ignore this brush so the body can leave.
    if shape.contains_centre(from) {
        return true;
    }
    let (end, touched) = shape.slide(from, to - from, r);
    if !touched {
        return true;
    }
    let target = new.to_world(end);
    let mut pos = slide_by(solids, body.pos, target - p, body.radius);
    for _ in 0..WEDGE_ROUNDS {
        let local = new.to_local(pos.to_f64());
        let Some((depth, out)) = shape.overlap(local, r - SQUEEZE_SLACK) else {
            body.pos = pos;
            return true;
        };
        let step = new.apply(out) * (depth + SQUEEZE_SLACK + SKIN);
        pos = slide_by(solids, pos, step, body.radius);
    }
    let local = new.to_local(pos.to_f64());
    let clear = shape.overlap(local, r - SQUEEZE_SLACK).is_none();
    if clear {
        body.pos = pos;
    }
    clear
}

/// One unit of time, so the velocity is simply the displacement.
fn slide_by(solids: Solids, pos: Vec3, by: Vec3d, radius: f32) -> Vec3 {
    slide_move(solids, pos, by.to_f32(), 1.0, radius).pos
}
