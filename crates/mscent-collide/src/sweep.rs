//! Sweeping a sphere through the BSP's solid cells and the voxel surfaces.
//!
//! The tree is walked front to back, pruning every subtree the swept capsule
//! cannot reach. Each solid leaf reached is rebuilt as a convex cell from the
//! planes on the path, and the exact cell sweep (faces pushed out by the
//! radius, rounded edges, rounded vertices) picks the earliest contact. The
//! voxel triangles are swept with the same contact model (`voxel.rs`) and the
//! earlier of the two wins.

use crate::cell::{Cell, CellPlane};
use crate::mover::MoverSolid;
use crate::prim::Contact;
use crate::tuning::{PRUNE_MARGIN, SKIN};
use crate::voxel;
use mscent_math::{Vec3, Vec3d};
use mscent_world::bsp::MAX_DEPTH;
use mscent_world::voxel::VoxelWorld;
use mscent_world::{Bsp, Child, LeafKind};

/// Outcome of a sphere sweep, in `f32` runtime precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Trace {
    /// How much of the move completed, `0.0..=1.0`.
    pub fraction: f32,
    /// Where the centre ends up. After a hit it is held `SKIN` off the surface.
    pub end: Vec3,
    /// Unit surface normal at the contact; zero when nothing was hit.
    pub normal: Vec3,
    /// True if the move was cut short.
    pub hit: bool,
    /// True if the start was inside solid (or the input was invalid); the
    /// sphere does not move.
    pub start_solid: bool,
    /// The mover struck, if the contact is with one rather than the world.
    pub mover: Option<usize>,
}

/// Everything solid: the world, its voxel surfaces if it has any, and the
/// movers at their current poses.
#[derive(Clone, Copy, Debug)]
pub struct Solids<'a> {
    bsp: &'a Bsp,
    voxels: Option<&'a VoxelWorld>,
    movers: &'a [MoverSolid],
    skip: Option<usize>,
}

impl<'a> Solids<'a> {
    pub fn new(bsp: &'a Bsp, movers: &'a [MoverSolid]) -> Self {
        Self {
            bsp,
            voxels: None,
            movers,
            skip: None,
        }
    }

    /// The same, with the surfaces of voxel volumes solid too. `voxels` must
    /// have been built against `bsp`.
    #[must_use]
    pub fn with_voxels(self, voxels: &'a VoxelWorld) -> Self {
        Self {
            voxels: Some(voxels),
            ..self
        }
    }

    /// The same world with other movers.
    pub(crate) fn over<'b>(self, movers: &'b [MoverSolid]) -> Solids<'b>
    where
        'a: 'b,
    {
        Solids {
            bsp: self.bsp,
            voxels: self.voxels,
            movers,
            skip: None,
        }
    }

    /// True if `p` is inside the world: BSP solid, or rock of a voxel volume.
    /// Movers are not counted.
    pub fn is_solid(&self, p: Vec3d) -> bool {
        self.bsp.is_solid(p) || self.voxels.is_some_and(|v| v.is_solid(p))
    }

    /// The same, ignoring one mover.
    pub(crate) fn without(self, index: usize) -> Self {
        Self {
            skip: Some(index),
            ..self
        }
    }
}

impl<'a> From<&'a Bsp> for Solids<'a> {
    fn from(bsp: &'a Bsp) -> Self {
        Self::new(bsp, &[])
    }
}

/// Double-precision trace used internally so slide iterations do not round
/// between bumps.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TraceD {
    pub fraction: f64,
    pub end: Vec3d,
    pub normal: Vec3d,
    pub hit: bool,
    pub start_solid: bool,
    pub mover: Option<usize>,
}

impl TraceD {
    fn blocked(from: Vec3d) -> Self {
        Self {
            fraction: 0.0,
            end: from,
            normal: Vec3d::ZERO,
            hit: true,
            start_solid: true,
            mover: None,
        }
    }

    pub fn to_f32(self) -> Trace {
        Trace {
            fraction: self.fraction as f32,
            end: self.end.to_f32(),
            normal: self.normal.to_f32(),
            hit: self.hit,
            start_solid: self.start_solid,
            mover: self.mover,
        }
    }
}

/// Sweeps a sphere of `radius` whose centre moves from `from` to `to`.
/// Invalid input (non-finite values, negative radius) yields a blocked trace
/// rather than a panic.
pub fn sweep_sphere<'a>(solids: impl Into<Solids<'a>>, from: Vec3, to: Vec3, radius: f32) -> Trace {
    let d = (from.to_f64(), to.to_f64(), f64::from(radius));
    sweep_sphere_d(solids.into(), d.0, d.1, d.2).to_f32()
}

pub(crate) fn sweep_sphere_d(solids: Solids, from: Vec3d, to: Vec3d, radius: f64) -> TraceD {
    let bsp = solids.bsp;
    if !from.is_finite() || !to.is_finite() || !radius.is_finite() || radius < 0.0 {
        return TraceD::blocked(from);
    }
    if solids.is_solid(from) {
        return TraceD::blocked(from);
    }
    let mut walk = Walk {
        bsp,
        from,
        to,
        d: to - from,
        radius,
        best: None,
        planes: Vec::new(),
    };
    walk.visit(bsp.root(), 0);
    let (best, d) = (walk.best, walk.d);
    let best = nearest_voxel(solids, from, d, radius, best);
    let (best, mover) = nearest_mover(solids, from, to, d, radius, best);
    match best {
        None => TraceD {
            fraction: 1.0,
            end: to,
            normal: Vec3d::ZERO,
            hit: false,
            start_solid: false,
            mover: None,
        },
        Some(c) => TraceD {
            fraction: c.t,
            // Backing off along the normal (not the path) keeps the sphere
            // clear of the surface even for grazing, near-parallel moves.
            end: from + d * c.t + c.normal * SKIN,
            normal: c.normal,
            hit: true,
            start_solid: false,
            mover,
        },
    }
}

/// The earlier of `world` and the first contact with a voxel surface.
fn nearest_voxel(
    solids: Solids,
    from: Vec3d,
    d: Vec3d,
    radius: f64,
    world: Option<Contact>,
) -> Option<Contact> {
    let Some(voxels) = solids.voxels else {
        return world;
    };
    let limit = world.map_or(1.0, |b| b.t);
    let found = voxel::sweep(voxels, from, d, radius, limit);
    found.filter(|c| world.is_none_or(|b| c.t < b.t)).or(world)
}

/// The earliest contact once movers are counted, and which mover it is on.
fn nearest_mover(
    solids: Solids,
    from: Vec3d,
    to: Vec3d,
    d: Vec3d,
    radius: f64,
    world: Option<Contact>,
) -> (Option<Contact>, Option<usize>) {
    let mut best = world;
    let mut struck = None;
    for (i, mover) in solids.movers.iter().enumerate() {
        if solids.skip == Some(i) || !mover.near_segment(from, to, radius) {
            continue;
        }
        let limit = best.map_or(1.0, |b| b.t);
        let found = mover.sweep(from, d, radius, limit);
        if let Some(c) = found.filter(|c| best.is_none_or(|b| c.t < b.t)) {
            best = Some(c);
            struck = Some(i);
        }
    }
    (best, struck)
}

struct Walk<'a> {
    bsp: &'a Bsp,
    from: Vec3d,
    to: Vec3d,
    d: Vec3d,
    radius: f64,
    best: Option<Contact>,
    planes: Vec<CellPlane>,
}

impl Walk<'_> {
    fn visit(&mut self, child: Child, depth: usize) {
        if depth > MAX_DEPTH + 1 {
            return;
        }
        match child {
            Child::Leaf(id) => {
                if self.bsp.leaf_kind(id) == LeafKind::Solid {
                    self.test_cell();
                }
            }
            Child::Node(id) => {
                let Some(node) = self.bsp.node(id) else {
                    return;
                };
                let (d0, d1) = (
                    node.plane.signed_distance(self.from),
                    node.plane.signed_distance(self.to),
                );
                let reach = self.radius + PRUNE_MARGIN;
                if d0.max(d1) + reach > 0.0 {
                    // Front half-space `n.p > d`, as an outward cell plane.
                    let outward = CellPlane {
                        normal: -node.plane.normal,
                        dist: -node.plane.dist,
                    };
                    self.descend(outward, node.front, depth);
                }
                if d0.min(d1) - reach <= 0.0 {
                    let outward = CellPlane {
                        normal: node.plane.normal,
                        dist: node.plane.dist,
                    };
                    self.descend(outward, node.back, depth);
                }
            }
        }
    }

    fn descend(&mut self, plane: CellPlane, child: Child, depth: usize) {
        self.planes.push(plane);
        self.visit(child, depth + 1);
        self.planes.pop();
    }

    fn test_cell(&mut self) {
        let limit = self.best.map_or(1.0, |b| b.t);
        let cell = Cell::build(&self.planes);
        let found = cell.sweep(self.from, self.d, self.radius, limit);
        if let Some(c) = found.filter(|c| self.best.is_none_or(|b| c.t < b.t)) {
            self.best = Some(c);
        }
    }
}
