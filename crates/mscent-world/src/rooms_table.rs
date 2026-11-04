//! The table of steps between rooms: for every pair, the smallest worst portal
//! gap over the chains of portals between them (see `rooms.rs`).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use super::{MAX_LIGHT_REACH, NEVER, NO_ROOM, Parts, STEPS_PER_UNIT};
use crate::bsp::LeafId;
use crate::portals::Portals;
use mscent_math::{Aabb, Vec3};

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

pub(super) fn steps_of(parts: &Parts, portals: &Portals) -> Vec<u16> {
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
