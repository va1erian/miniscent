//! Scorch decals: a ring of marks left where a bolt struck a wall.
//!
//! A decal is a square on the struck plane, cut to the face it hit so a mark
//! near a corner does not climb the next wall. The cut happens once, when the
//! decal is made; storage is a fixed array, so a shot allocates nothing and
//! the 65th mark replaces the first.

use mscent_math::epsilon::MIN_LENGTH_F32;
use mscent_math::{Vec3, Vec3d};
use mscent_render::Camera;
use mscent_render::particles::{DecalVertex, MAX_DECAL_POINTS, Mask, decal_from_world};
use mscent_world::Face;

use super::budget::Sink;
use super::collect::FogView;
use super::recipes::Mark;

/// Marks kept; older ones are replaced.
pub const MAX_DECALS: usize = 64;
/// One fewer than a draw holds, because the near plane can add a point.
const MAX_VERTS: usize = MAX_DECAL_POINTS - 1;
/// Edges of a face an outline keeps. A face with more is not clipped against.
pub const MAX_EDGES: usize = 24;
/// Noisy masks to choose from, so neighbouring marks differ.
const MASK_VARIANTS: usize = 4;
/// What decals may fill in one frame, in multiples of the frame's pixels.
/// Marks pile up on a wall someone is standing at, and they are never thinned
/// by the budget, so this is their own bound; the oldest go first.
const MAX_SCREENS: f32 = 0.5;
/// Full strength of a fog fade or a mark's darkness.
const OPAQUE: u32 = 256;

const NO_VERTEX: DecalVertex = DecalVertex {
    pos: Vec3::ZERO,
    s: 0.0,
    t: 0.0,
};

/// A half-space `normal . p >= dist`, the inside of one edge of a face.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Edge {
    normal: Vec3,
    dist: f32,
}

/// The plane of a face and the half-spaces of its edges, from the level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceOutline {
    normal: Vec3,
    dist: f32,
    edges: [Edge; MAX_EDGES],
    len: u8,
}

impl FaceOutline {
    /// `None` for a degenerate face or one with more than [`MAX_EDGES`] edges
    /// once repeated directions are merged; the decal is then left unclipped.
    pub fn of(face: &Face) -> Option<Self> {
        let n = face.plane.normal.normalized()?;
        let verts = face.polygon.vertices();
        let mut out = Self {
            normal: n.to_f32(),
            dist: face.plane.dist as f32,
            edges: [Edge {
                normal: Vec3::ZERO,
                dist: 0.0,
            }; MAX_EDGES],
            len: 0,
        };
        let mut prev = *verts.last()?;
        for &v in verts {
            out.push_edge(n, prev, v)?;
            prev = v;
        }
        (out.len >= 3 && out.dist.is_finite()).then_some(out)
    }

    /// The inward half-space of the edge `a` to `b`. A zero-length edge or a
    /// repeat of the previous direction (collinear vertices) adds nothing.
    fn push_edge(&mut self, n: Vec3d, a: Vec3d, b: Vec3d) -> Option<()> {
        let Some(m) = n.cross(b - a).normalized() else {
            return Some(());
        };
        let edge = Edge {
            normal: m.to_f32(),
            dist: m.dot(a) as f32,
        };
        let len = usize::from(self.len);
        let repeat =
            len > 0 && (self.edges[len - 1].normal - edge.normal).length() < MIN_LENGTH_F32;
        if repeat {
            return Some(());
        }
        *self.edges.get_mut(len)? = edge;
        self.len += 1;
        Some(())
    }

    fn edges(&self) -> &[Edge] {
        &self.edges[..usize::from(self.len).min(MAX_EDGES)]
    }
}

/// One mark: a convex polygon on a wall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decal {
    verts: [DecalVertex; MAX_VERTS],
    len: u8,
    darkness: u16,
    mask: u8,
}

impl Decal {
    const EMPTY: Self = Self {
        verts: [NO_VERTEX; MAX_VERTS],
        len: 0,
        darkness: 0,
        mask: 0,
    };

    /// A mark at `at` on the plane with unit `normal`, or on the plane of
    /// `face` when it is given, cut to the face. `None` for a position or
    /// normal that is not finite, or when nothing of the mark is on the face.
    pub fn new(at: Vec3, normal: Vec3, mark: Mark, face: Option<&FaceOutline>) -> Option<Self> {
        if !(at.is_finite() && normal.is_finite()) {
            return None;
        }
        let (n, at) = match face {
            // On the face's own plane, however far off the shot's end point.
            Some(f) => (f.normal, at - f.normal * (f.normal.dot(at) - f.dist)),
            None => (normal.normalized()?, at),
        };
        let axis = least_aligned(n);
        let u = n.cross(axis).normalized()?;
        let v = n.cross(u);
        let corner = |su: f32, sv: f32| DecalVertex {
            pos: at + u * (su * mark.radius) + v * (sv * mark.radius),
            s: (su + 1.0) * 0.5,
            t: (sv + 1.0) * 0.5,
        };
        let mut decal = Self::EMPTY;
        decal.verts[..4].copy_from_slice(&[
            corner(-1.0, -1.0),
            corner(1.0, -1.0),
            corner(1.0, 1.0),
            corner(-1.0, 1.0),
        ]);
        decal.len = 4;
        decal.darkness = mark.darkness;
        if let Some(f) = face {
            decal.clip(f.edges());
        }
        (decal.len >= 3 && decal.verts[..4].iter().all(|v| v.pos.is_finite())).then_some(decal)
    }

    /// Cut to each half-space in turn. A cut that would not fit is skipped,
    /// which leaves the mark a little over that edge and never invalid.
    fn clip(&mut self, edges: &[Edge]) {
        for e in edges {
            let mut out = [NO_VERTEX; MAX_VERTS + 1];
            let n = clip_edge(&self.verts[..usize::from(self.len)], e, &mut out);
            if n < 3 {
                self.len = 0;
                return;
            }
            if n <= MAX_VERTS {
                self.verts[..n].copy_from_slice(&out[..n]);
                self.len = n as u8;
            }
        }
    }

    /// Mean of the corners, for tests and for judging distance.
    pub fn centre(&self) -> Vec3 {
        let pts = &self.verts[..usize::from(self.len)];
        let sum = pts.iter().fold(Vec3::ZERO, |a, v| a + v.pos);
        sum * (1.0 / pts.len().max(1) as f32)
    }

    pub fn vertices(&self) -> &[DecalVertex] {
        &self.verts[..usize::from(self.len)]
    }
}

/// The world axis a normal is least along, so crossing with it is well
/// conditioned.
fn least_aligned(n: Vec3) -> Vec3 {
    let (x, y, z) = (n.x.abs(), n.y.abs(), n.z.abs());
    if x <= y && x <= z {
        Vec3::new(1.0, 0.0, 0.0)
    } else if y <= z {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    }
}

/// Add a point to a clipped polygon; one past the end is dropped.
fn append(out: &mut [DecalVertex], n: &mut usize, v: DecalVertex) {
    if let Some(slot) = out.get_mut(*n) {
        *slot = v;
        *n += 1;
    }
}

/// Sutherland-Hodgman for one half-space. A convex polygon gains at most one
/// point, so `out` is large enough whenever `input` fits [`MAX_VERTS`].
fn clip_edge(input: &[DecalVertex], e: &Edge, out: &mut [DecalVertex; MAX_VERTS + 1]) -> usize {
    let Some(&last) = input.last() else { return 0 };
    let side = |v: &DecalVertex| e.normal.dot(v.pos) - e.dist;
    let (mut prev, mut prev_d) = (last, side(&last));
    let mut n = 0;
    for &cur in input {
        let cur_d = side(&cur);
        if (cur_d >= 0.0) != (prev_d >= 0.0) {
            let k = prev_d / (prev_d - cur_d);
            let lerp = |a: f32, b: f32| a + (b - a) * k;
            let hit = DecalVertex {
                pos: prev.pos.lerp(cur.pos, k),
                s: lerp(prev.s, cur.s),
                t: lerp(prev.t, cur.t),
            };
            append(out, &mut n, hit);
        }
        if cur_d >= 0.0 {
            append(out, &mut n, cur);
        }
        (prev, prev_d) = (cur, cur_d);
    }
    n
}

/// The ring of marks, oldest replaced first. Fixed size.
pub struct Decals {
    slots: [Decal; MAX_DECALS],
    /// Where the next one goes.
    next: usize,
    live: usize,
}

impl Default for Decals {
    fn default() -> Self {
        Self::new()
    }
}

impl Decals {
    pub fn new() -> Self {
        Self {
            slots: [Decal::EMPTY; MAX_DECALS],
            next: 0,
            live: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    pub fn clear(&mut self) {
        self.next = 0;
        self.live = 0;
    }

    /// Add a mark, replacing the oldest when the ring is full. The mask comes
    /// from the slot, so marks beside each other differ.
    pub fn push(&mut self, mut decal: Decal) {
        decal.mask = (self.next % MASK_VARIANTS) as u8;
        if let Some(slot) = self.slots.get_mut(self.next) {
            *slot = decal;
            self.next = (self.next + 1) % MAX_DECALS;
            self.live = (self.live + 1).min(MAX_DECALS);
        }
    }

    /// Live marks, oldest first.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &Decal> {
        let first = (self.next + MAX_DECALS - self.live) % MAX_DECALS;
        (0..self.live).filter_map(move |i| self.slots.get((first + i) % MAX_DECALS))
    }

    /// Appends each visible mark to the frame's draws, newest first, until
    /// they fill [`MAX_SCREENS`] of the screen. Fog fades a mark with its
    /// distance as it does an "over" particle's tint, because a black mark in
    /// fog would stand out from the haze around it.
    pub fn collect(&self, cam: &Camera, fog: &FogView, out: &mut Sink) {
        let mut room = MAX_SCREENS * out.width * out.height;
        for d in self.iter().rev() {
            let depth = -cam.world_to_camera(d.centre()).z;
            let Some(fade) = fog.fade(depth) else {
                continue;
            };
            let darkness = (u32::from(d.darkness) * fade / OPAQUE) as u16;
            let mask = Mask::Puff(d.mask);
            let Some(draw) = decal_from_world(cam, d.vertices(), darkness, mask) else {
                continue;
            };
            room -= draw.screen_area(out.width, out.height);
            if room < 0.0 {
                break;
            }
            out.decal(draw);
        }
    }
}

#[cfg(test)]
#[path = "../fx_decal_tests.rs"]
mod fx_decal_tests;
