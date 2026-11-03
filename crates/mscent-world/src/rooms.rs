//! Rooms: empty leaves merged where the BSP only cut open space, and a table of
//! how far apart the rooms are.
//!
//! The tree cuts a room with the planes of every wall that bounds it, so a
//! pillar, an alcove or a doorway splits one room into several leaves. A
//! plane that the portals cover at least half of, in each leaf it cuts, is
//! open space cut in two; a doorway covers a small part of the wall on its
//! plane, so it keeps its rooms apart. Leaves joined by open portals are one
//! room.
//!
//! The table answers one question for a light: from a light in room `a`, which
//! rooms can it reach with radius `r`? It holds, for every pair, the smallest
//! over the portal chains from `a` to `b` of the largest gap between `a`'s box
//! and a portal on the chain, in sixteenths of a unit and rounded down. A chain
//! a light of radius `r` floods through needs every portal within `r` of the
//! light, and the light is inside `a`'s box, so the table never says no where
//! that flood said yes. It is the flood done once at compile time for every
//! position of the room, which is why a light that moves needs none.
//!
//! The rooms are derived from the tree and the portals, never read from a
//! file, so a level file cannot forge them. [`Rooms::from_parts`] is the one
//! door every table goes through, and it refuses what the builder cannot make.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::bsp::{Bsp, Child, LeafId, LeafKind};
use crate::portals::{Portal, Portals};
use crate::{csg, polytope};
use mscent_math::{Aabb, DPlane, Vec3, epsilon};

/// Most rooms a level keeps; the table is the square of this many entries.
pub const MAX_ROOMS: usize = 4096;
/// Farthest a dynamic light reaches, in world units. A pair farther apart than
/// this holds [`NEVER`].
pub const MAX_LIGHT_REACH: f32 = 64.0;
/// Table steps in one world unit.
pub const STEPS_PER_UNIT: f32 = 16.0;
/// A table entry for rooms no light reaches from one to the other.
pub const NEVER: u16 = u16::MAX;
/// The share of a leaf's face on a portal's plane the portal must cover, in
/// both leaves, for the portal to be open space.
const OPEN_SHARE: f64 = 0.5;
/// What a box grows by, for the `f32` planes `leaf_at_f32` descends by.
const BOX_SLACK: f32 = 0.01;
const NO_ROOM: u32 = u32::MAX;

/// Why a hand-built set of rooms was rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomError {
    /// More rooms than [`MAX_ROOMS`].
    TooMany,
    /// The table is not `count` by `count`.
    BadTable,
    /// A leaf names a room that does not exist.
    BadRoom,
    /// A room is not at distance zero from itself.
    BadDiagonal,
}

/// The rooms of a level and the table of steps between them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rooms {
    of_leaf: Vec<u32>,
    count: u32,
    steps: Vec<u16>,
}

impl Rooms {
    /// Validates and wraps raw parts: `of_leaf` names a room below `count` for
    /// an empty leaf and anything at or past it for a solid one, `steps` holds
    /// `count * count` entries row by row, and each room is at zero from itself.
    pub fn from_parts(of_leaf: Vec<u32>, count: usize, steps: Vec<u16>) -> Result<Self, RoomError> {
        if count > MAX_ROOMS {
            return Err(RoomError::TooMany);
        }
        if count.checked_mul(count) != Some(steps.len()) {
            return Err(RoomError::BadTable);
        }
        // `count` is at most `MAX_ROOMS`, so it fits.
        let count = u32::try_from(count).map_err(|_| RoomError::TooMany)?;
        let of_leaf = of_leaf
            .into_iter()
            .map(|r| {
                if r < count || r == NO_ROOM {
                    Ok(r)
                } else {
                    Err(RoomError::BadRoom)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        let rooms = Self {
            of_leaf,
            count,
            steps,
        };
        let diagonal = (0..count).all(|r| rooms.step(r, r) == Some(0));
        if diagonal {
            Ok(rooms)
        } else {
            Err(RoomError::BadDiagonal)
        }
    }

    /// The rooms of `bsp`. A level with more rooms than [`MAX_ROOMS`] takes one
    /// room for each island of leaves, and one with more islands than that
    /// reaches nothing, which is the dark side of being wrong.
    pub fn build(bsp: &Bsp, portals: &Portals) -> Self {
        let mut cells = cells_of(bsp);
        measure_openings(portals, &mut cells);
        let mut parts = Parts::open(bsp, portals, &cells);
        if parts.count > MAX_ROOMS {
            parts = Parts::islands(bsp, portals, &cells);
        }
        if parts.count > MAX_ROOMS {
            return Self::default();
        }
        let steps = steps_of(&parts, portals);
        Self::from_parts(parts.of_leaf, parts.count, steps).unwrap_or_default()
    }

    /// How many rooms there are.
    pub fn count(&self) -> usize {
        self.count as usize
    }

    /// The room `leaf` is in; `None` for a solid leaf or one out of range.
    pub fn room_of(&self, leaf: LeafId) -> Option<u32> {
        self.of_leaf
            .get(leaf.0 as usize)
            .copied()
            .filter(|&r| r < self.count)
    }

    /// Bytes the table takes.
    pub fn table_bytes(&self) -> usize {
        self.steps.len() * 2
    }

    /// The table entry from room `from` to room `to`.
    pub fn step(&self, from: u32, to: u32) -> Option<u16> {
        self.row(from)?.get(to as usize).copied()
    }

    /// The entries from room `from` to every room; `None` for no such room.
    pub fn row(&self, from: u32) -> Option<&[u16]> {
        let n = self.count as usize;
        let start = (from as usize).checked_mul(n)?;
        self.steps.get(start..start.checked_add(n)?)
    }

    /// Whether a light of `radius` in room `from` reaches room `to`. The
    /// distance is rounded down, so this errs towards yes.
    pub fn reaches(&self, from: u32, to: u32, radius: f32) -> bool {
        self.step(from, to)
            .is_some_and(|s| s != NEVER && f32::from(s) < radius * STEPS_PER_UNIT)
    }
}

/// One empty leaf's cell: the box around it and, for each plane of the tree
/// that bounds it, the plane (pointing out of the cell) and the area of the
/// cell's face on it.
struct Cell {
    bounds: Aabb,
    sides: Vec<Side>,
}

/// A plane bounding a cell: the plane, the area of the cell's face on it, and
/// how much of that the portals cover.
struct Side {
    plane: DPlane,
    whole: f64,
    open: f64,
}

impl Cell {
    fn side(&self, outward: &DPlane) -> Option<(f64, f64)> {
        let s = self.sides.iter().find(|s| same_plane(&s.plane, outward))?;
        Some((s.whole, s.open))
    }

    fn open_mut(&mut self, outward: &DPlane) -> Option<&mut f64> {
        let s = self
            .sides
            .iter_mut()
            .find(|s| same_plane(&s.plane, outward))?;
        Some(&mut s.open)
    }
}

fn cells_of(bsp: &Bsp) -> Vec<Option<Cell>> {
    let mut cells: Vec<Option<Cell>> = (0..bsp.leaves().len()).map(|_| None).collect();
    walk(bsp, bsp.root(), &mut Vec::new(), &mut cells);
    cells
}

/// Visits leaves keeping `bounding`: the planes, pointing outward, of the cell.
fn walk(bsp: &Bsp, child: Child, bounding: &mut Vec<DPlane>, out: &mut [Option<Cell>]) {
    match child {
        Child::Leaf(id) => {
            if bsp.leaf_kind(id) == LeafKind::Empty
                && let Some(slot) = out.get_mut(id.0 as usize)
            {
                *slot = Some(cell_of(bounding));
            }
        }
        Child::Node(id) => {
            let Some(node) = bsp.node(id) else { return };
            bounding.push(node.plane.flipped());
            walk(bsp, node.front, bounding, out);
            bounding.pop();
            bounding.push(node.plane);
            walk(bsp, node.back, bounding, out);
            bounding.pop();
        }
    }
}

fn cell_of(bounding: &[DPlane]) -> Cell {
    // The world box closes cells the tree leaves open.
    let mut planes = bounding.to_vec();
    planes.extend(csg::world_box());
    let mut bounds = Aabb::empty();
    let mut sides = Vec::with_capacity(bounding.len());
    for (i, poly) in polytope::faces(&planes) {
        for v in poly.vertices() {
            bounds = bounds.including(v.to_f32());
        }
        if let Some(&plane) = bounding.get(i) {
            sides.push(Side {
                plane,
                whole: poly.area(),
                open: 0.0,
            });
        }
    }
    Cell {
        bounds: bounds.expanded(BOX_SLACK),
        sides,
    }
}

/// Leaves grouped into rooms and the box around each.
struct Parts {
    of_leaf: Vec<u32>,
    count: usize,
    boxes: Vec<Aabb>,
}

impl Parts {
    /// Rooms of leaves joined by open portals.
    fn open(bsp: &Bsp, portals: &Portals, cells: &[Option<Cell>]) -> Self {
        let mut groups = Groups::new(bsp.leaves().len());
        for p in portals.portals() {
            if is_open(p, cells) {
                groups.join(p.front.0 as usize, p.back.0 as usize);
            }
        }
        Self::from_groups(bsp, cells, |leaf| groups.find(leaf))
    }

    /// Rooms of leaves a chain of portals joins.
    fn islands(bsp: &Bsp, portals: &Portals, cells: &[Option<Cell>]) -> Self {
        let mut groups = Groups::new(bsp.leaves().len());
        for p in portals.portals() {
            groups.join(p.front.0 as usize, p.back.0 as usize);
        }
        Self::from_groups(bsp, cells, |leaf| groups.find(leaf))
    }

    /// Numbers the groups of the empty leaves in the order of their lowest leaf.
    fn from_groups(
        bsp: &Bsp,
        cells: &[Option<Cell>],
        mut group: impl FnMut(usize) -> usize,
    ) -> Self {
        let mut number = vec![NO_ROOM; bsp.leaves().len()];
        let mut of_leaf = vec![NO_ROOM; bsp.leaves().len()];
        let mut boxes: Vec<Aabb> = Vec::new();
        for (leaf, cell) in cells.iter().enumerate() {
            let Some(cell) = cell else { continue };
            let root = group(leaf);
            let room = match number.get(root).copied() {
                Some(r) if r != NO_ROOM => r,
                _ => {
                    // Every leaf is a leaf of the tree, so the count fits.
                    let r = u32::try_from(boxes.len()).unwrap_or(NO_ROOM - 1);
                    boxes.push(Aabb::empty());
                    if let Some(slot) = number.get_mut(root) {
                        *slot = r;
                    }
                    r
                }
            };
            if let (Some(b), Some(slot)) = (boxes.get_mut(room as usize), of_leaf.get_mut(leaf)) {
                *b = b.union(cell.bounds);
                *slot = room;
            }
        }
        Self {
            of_leaf,
            count: boxes.len(),
            boxes,
        }
    }
}

/// Union-find over leaf numbers.
struct Groups(Vec<usize>);

impl Groups {
    fn new(n: usize) -> Self {
        Self((0..n).collect())
    }

    fn find(&mut self, mut i: usize) -> usize {
        while let Some(&up) = self.0.get(i) {
            if up == i {
                break;
            }
            // Halving keeps the trees shallow.
            let grand = self.0.get(up).copied().unwrap_or(up);
            if let Some(slot) = self.0.get_mut(i) {
                *slot = grand;
            }
            i = grand;
        }
        i
    }

    fn join(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        let (low, high) = (ra.min(rb), ra.max(rb));
        if let Some(slot) = self.0.get_mut(high) {
            *slot = low;
        }
    }
}

/// The two leaves of `portal` with the plane of the portal pointing out of each.
fn ends(portal: &Portal) -> Option<[(LeafId, DPlane); 2]> {
    let plane = portal.polygon.plane()?;
    // The plane points into the front leaf, so out of the back one.
    Some([(portal.front, plane.flipped()), (portal.back, plane)])
}

/// Adds the area of each portal to the open part of its plane in both leaves.
fn measure_openings(portals: &Portals, cells: &mut [Option<Cell>]) {
    for portal in portals.portals() {
        let area = portal.polygon.area();
        for (leaf, outward) in ends(portal).into_iter().flatten() {
            let cell = cells.get_mut(leaf.0 as usize).and_then(Option::as_mut);
            if let Some(open) = cell.and_then(|c| c.open_mut(&outward)) {
                *open += area;
            }
        }
    }
}

/// Whether `portal` is on a plane that is at least [`OPEN_SHARE`] open in both
/// of its leaves, counting every portal of the leaf on that plane.
fn is_open(portal: &Portal, cells: &[Option<Cell>]) -> bool {
    let Some(ends) = ends(portal) else {
        return false;
    };
    ends.into_iter().all(|(leaf, outward)| {
        let cell = cells.get(leaf.0 as usize).and_then(Option::as_ref);
        let side = cell.and_then(|c| c.side(&outward));
        side.is_some_and(|(whole, open)| open >= OPEN_SHARE * whole)
    })
}

fn same_plane(a: &DPlane, b: &DPlane) -> bool {
    let eps = epsilon::PLANE_EPSILON_F64;
    (a.normal - b.normal).length() <= eps && (a.dist - b.dist).abs() <= eps
}

/// The gap between two boxes, zero where they touch or overlap.
fn box_gap(a: &Aabb, b: &Aabb) -> f32 {
    let gap = |lo: f32, hi: f32, olo: f32, ohi: f32| (olo - hi).max(lo - ohi).max(0.0);
    Vec3::new(
        gap(a.min.x, a.max.x, b.min.x, b.max.x),
        gap(a.min.y, a.max.y, b.min.y, b.max.y),
        gap(a.min.z, a.max.z, b.min.z, b.max.z),
    )
    .length()
}

/// A portal between two rooms: the rooms and the box around it.
struct Edge {
    rooms: [u32; 2],
    bounds: Aabb,
}

fn edges_of(parts: &Parts, portals: &Portals) -> Vec<Edge> {
    let room = |leaf: LeafId| {
        parts
            .of_leaf
            .get(leaf.0 as usize)
            .copied()
            .unwrap_or(NO_ROOM)
    };
    portals
        .portals()
        .iter()
        .filter_map(|p| {
            let rooms = [room(p.front), room(p.back)];
            let bounds = p
                .polygon
                .vertices()
                .iter()
                .fold(Aabb::empty(), |b, v| b.including(v.to_f32()));
            (rooms[0] != rooms[1] && rooms.iter().all(|&r| r != NO_ROOM))
                .then_some(Edge { rooms, bounds })
        })
        .collect()
}

/// For each room, the portals that touch it, by index into `edges`.
fn touching(count: usize, edges: &[Edge]) -> Vec<Vec<u32>> {
    let mut out = vec![Vec::new(); count];
    for (i, e) in (0u32..).zip(edges) {
        for r in e.rooms {
            if let Some(list) = out.get_mut(r as usize) {
                list.push(i);
            }
        }
    }
    out
}

fn steps_of(parts: &Parts, portals: &Portals) -> Vec<u16> {
    let edges = edges_of(parts, portals);
    let touch = touching(parts.count, &edges);
    let mut table = vec![NEVER; parts.count.saturating_mul(parts.count)];
    for (from, home) in parts.boxes.iter().enumerate() {
        let start = from.saturating_mul(parts.count);
        if let Some(row) = table.get_mut(start..start.saturating_add(parts.count)) {
            narrowest_chains(from, home, (&edges, &touch), row);
        }
    }
    table
}

/// Fills `row` with the step from room `from` to each room: Dijkstra where a
/// chain costs its worst portal, so the first time a room comes off the heap is
/// the best chain to it.
fn narrowest_chains(
    from: usize,
    home: &Aabb,
    (edges, touch): (&[Edge], &[Vec<u32>]),
    row: &mut [u16],
) {
    let limit = (MAX_LIGHT_REACH * STEPS_PER_UNIT) as u32;
    let mut heap = BinaryHeap::new();
    // `from` is below `MAX_ROOMS`.
    heap.push(Reverse((0u32, u32::try_from(from).unwrap_or(0))));
    while let Some(Reverse((cost, room))) = heap.pop() {
        let Some(slot) = row.get_mut(room as usize) else {
            continue;
        };
        if *slot != NEVER {
            continue;
        }
        *slot = u16::try_from(cost).unwrap_or(NEVER);
        for &e in touch.get(room as usize).map_or(&[][..], Vec::as_slice) {
            let Some(edge) = edges.get(e as usize) else {
                continue;
            };
            let across = (box_gap(home, &edge.bounds) * STEPS_PER_UNIT) as u32;
            let next = cost.max(across);
            let other = if edge.rooms[0] == room {
                edge.rooms[1]
            } else {
                edge.rooms[0]
            };
            if next < limit {
                heap.push(Reverse((next, other)));
            }
        }
    }
}

#[cfg(test)]
#[path = "rooms_tests.rs"]
mod rooms_tests;
