//! The portal flood that decided a light's reach before the room table, kept
//! as the oracle the table is held to: whatever it reaches, the table must
//! reach too (`reach_tests.rs`). It floods from the light's leaf through the
//! portals the light is in front of and within the radius of.

use mscent_math::Vec3;
use mscent_world::{LeafId, LeafKind};

use super::{SIDE_SLACK, Scene};

/// The leaves a flood reached and the faces in them that face the light.
/// Both are bit sets that keep their storage between lights, so placing
/// another light into one allocates only when the level is bigger than any
/// the set has held.
#[derive(Clone, Debug, Default)]
pub(super) struct Flood {
    leaves: Vec<u64>,
    faces: Vec<u64>,
    stack: Vec<u32>,
}

fn reset(bits: &mut Vec<u64>, count: usize) {
    bits.clear();
    bits.resize(count.div_ceil(64), 0);
}

/// Sets bit `i`; true if it was clear. Out-of-range bits are left alone.
fn set(bits: &mut [u64], i: usize) -> bool {
    match bits.get_mut(i / 64) {
        Some(word) => {
            let mask = 1u64 << (i % 64);
            let fresh = *word & mask == 0;
            *word |= mask;
            fresh
        }
        None => false,
    }
}

fn get(bits: &[u64], i: usize) -> bool {
    bits.get(i / 64).is_some_and(|w| w >> (i % 64) & 1 == 1)
}

impl Flood {
    /// Whether the light reached `leaf`.
    pub(super) fn has_leaf(&self, leaf: LeafId) -> bool {
        get(&self.leaves, leaf.0 as usize)
    }

    /// Whether face `face` is in a reached leaf, within the light's radius
    /// and on its lit side.
    pub(super) fn has_face(&self, face: usize) -> bool {
        get(&self.faces, face)
    }
}

/// Shortest distance from `p` to the box around `points`.
fn distance_to_box(points: &[Vec3], p: Vec3) -> f32 {
    let Some(&first) = points.first() else {
        return f32::INFINITY;
    };
    let (lo, hi) = points
        .iter()
        .fold((first, first), |(lo, hi), &q| (lo.min(q), hi.max(q)));
    let gap = |p: f32, lo: f32, hi: f32| (lo - p).max(p - hi).max(0.0);
    Vec3::new(
        gap(p.x, lo.x, hi.x),
        gap(p.y, lo.y, hi.y),
        gap(p.z, lo.z, hi.z),
    )
    .length()
}

impl Scene {
    /// Replaces `out` with what a flood from `pos` of `radius` reaches. A light
    /// that is not finite, has no positive radius, or sits in solid reaches
    /// nothing.
    pub(super) fn flood_reach(&self, pos: Vec3, radius: f32, out: &mut Flood) {
        reset(&mut out.leaves, self.bsp.leaves().len());
        reset(&mut out.faces, self.faces.len());
        out.stack.clear();
        if !pos.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let start = self.bsp.leaf_at_f32(pos);
        if self.bsp.leaf_kind(start) != LeafKind::Empty {
            return;
        }
        set(&mut out.leaves, start.0 as usize);
        out.stack.push(start.0);
        // Each leaf is pushed once, so the flood ends in at most one visit
        // per leaf whatever the portals look like.
        while let Some(leaf) = out.stack.pop() {
            self.mark_faces(LeafId(leaf), pos, radius, &mut out.faces);
            for &pi in self.portals.leaf_portals(LeafId(leaf)) {
                let Some(next) = self.light_crosses(pos, radius, LeafId(leaf), pi as usize) else {
                    continue;
                };
                if set(&mut out.leaves, next.0 as usize) {
                    out.stack.push(next.0);
                }
            }
        }
    }

    /// The leaf past portal `pi` if light from `pos` gets through it: the
    /// light is on `from`'s side and close enough.
    fn light_crosses(&self, pos: Vec3, radius: f32, from: LeafId, pi: usize) -> Option<LeafId> {
        let portal = self.portals.portals().get(pi)?;
        let target = portal.other(from)?;
        let info = self.portal_info.get(pi)?;
        let side = info.plane.signed_distance(pos.to_f64());
        let facing = if from == portal.front { side } else { -side };
        (facing >= -SIDE_SLACK && distance_to_box(&info.points, pos) < radius).then_some(target)
    }

    /// Marks the faces of `leaf` that face `pos` from less than `radius`.
    fn mark_faces(&self, leaf: LeafId, pos: Vec3, radius: f32, faces: &mut [u64]) {
        let Some(list) = self.leaf_faces.get(leaf.0 as usize) else {
            return;
        };
        for &fi in list {
            let Some(face) = self.faces.get(fi as usize) else {
                continue;
            };
            let d = face.plane.signed_distance(pos.to_f64());
            if d > 0.0 && d < f64::from(radius) {
                set(faces, fi as usize);
            }
        }
    }
}
