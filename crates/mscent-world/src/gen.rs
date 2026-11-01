//! Seeded generator of valid brush levels, for tests and fuzzing.
//!
//! Rooms sit one per cell of a coarse grid. A random spanning tree over the
//! grid, plus a few extra links, decides which neighbours get a corridor, so
//! the open space is connected by construction. Corridors are subtract
//! brushes; pillars are add brushes applied last. Everything is placed on
//! integer coordinates and pillars keep clear of the room centre cross and of
//! the walls, so a pillar can never seal a doorway or split a room.

use crate::brush::{Brush, Op};
use mscent_math::Vec3d;
use mscent_math::rng::Rng;

/// Distance between neighbouring cell origins.
const PITCH: i32 = 40;
/// Rooms stay this far from their cell edge, leaving 8 units of rock between
/// neighbours for a corridor to cross.
const MARGIN: i32 = 4;
const MAX_HALF: i32 = 16;
const MIN_HALF: i32 = 8;
const CORRIDOR_HALF_WIDTH: i32 = 2;
const CORRIDOR_HEIGHT: i32 = 6;
/// Pillars have a 2x2 footprint.
const PILLAR_HALF: i32 = 1;
const MAX_GRID: u32 = 8;

/// Knobs for [`generate`]. Out-of-range values are clamped, never rejected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenParams {
    /// Rooms along x, clamped to `1..=8`.
    pub cols: u32,
    /// Rooms along y, clamped to `1..=8`.
    pub rows: u32,
    /// Corridors added beyond the spanning tree, making loops.
    pub extra_links: u32,
    /// Chance (0 to 100) that a room gets a pillar.
    pub pillar_percent: u32,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            cols: 3,
            rows: 3,
            extra_links: 2,
            pillar_percent: 50,
        }
    }
}

#[derive(Clone, Copy)]
struct Room {
    cx: i32,
    cy: i32,
    hx: i32,
    hy: i32,
    height: i32,
}

type Link = (usize, usize);
type Boxes = Vec<([i32; 3], [i32; 3], Op)>;

struct Layout {
    rooms: Vec<Room>,
    links: Vec<Link>,
}

/// Brushes of a level: rooms and corridors (subtract), then pillars (add),
/// in application order. The same `seed` and `params` always give the same
/// brushes.
pub fn generate(seed: u64, params: &GenParams) -> Vec<Brush> {
    let mut rng = Rng::new(seed);
    let layout = make_layout(&mut rng, params);
    let mut boxes: Boxes = layout
        .rooms
        .iter()
        .map(|r| {
            (
                [r.cx - r.hx, r.cy - r.hy, 0],
                [r.cx + r.hx, r.cy + r.hy, r.height],
                Op::Subtract,
            )
        })
        .collect();
    for &(a, b) in &layout.links {
        if let (Some(ra), Some(rb)) = (layout.rooms.get(a), layout.rooms.get(b)) {
            corridor(ra, rb, &mut boxes);
        }
    }
    for r in &layout.rooms {
        if rng.below(100) < params.pillar_percent {
            boxes.push(pillar(&mut rng, r));
        }
    }
    boxes
        .iter()
        .zip(0u32..)
        .filter_map(|(&(lo, hi, op), order)| Brush::cuboid(vec3(lo), vec3(hi), op, order).ok())
        .collect()
}

/// A point inside every room, a unit above the floor, in room order. Useful
/// for spawning and for connectivity checks.
pub fn room_centers(seed: u64, params: &GenParams) -> Vec<Vec3d> {
    let mut rng = Rng::new(seed);
    make_layout(&mut rng, params)
        .rooms
        .iter()
        .map(|r| Vec3d::new(f64::from(r.cx), f64::from(r.cy), 1.0))
        .collect()
}

fn vec3(p: [i32; 3]) -> Vec3d {
    Vec3d::new(f64::from(p[0]), f64::from(p[1]), f64::from(p[2]))
}

fn make_layout(rng: &mut Rng, params: &GenParams) -> Layout {
    let cols = params.cols.clamp(1, MAX_GRID) as usize;
    let rows = params.rows.clamp(1, MAX_GRID) as usize;
    let mut rooms = Vec::with_capacity(cols * rows);
    for row in 0..rows {
        for col in 0..cols {
            rooms.push(make_room(rng, col, row));
        }
    }
    let mut links = spanning_tree(rng, cols, rows);
    add_extra_links(rng, cols, rows, params.extra_links, &mut links);
    Layout { rooms, links }
}

fn make_room(rng: &mut Rng, col: usize, row: usize) -> Room {
    let hx = half_size(rng);
    let hy = half_size(rng);
    let (ox, oy) = (cell_origin(col), cell_origin(row));
    Room {
        cx: ox + MARGIN + hx + jitter(rng, hx),
        cy: oy + MARGIN + hy + jitter(rng, hy),
        hx,
        hy,
        height: 8 + below_i32(rng, 5),
    }
}

fn below_i32(rng: &mut Rng, n: i32) -> i32 {
    i32::try_from(rng.below(u32::try_from(n).unwrap_or(1))).unwrap_or(0)
}

fn cell_origin(i: usize) -> i32 {
    // The grid is at most 8 wide, so the conversion cannot fail.
    i32::try_from(i).unwrap_or(0) * PITCH
}

fn half_size(rng: &mut Rng) -> i32 {
    MIN_HALF + below_i32(rng, MAX_HALF - MIN_HALF + 1)
}

/// Slack left in the cell after placing a room of half-size `half`.
fn jitter(rng: &mut Rng, half: i32) -> i32 {
    below_i32(rng, PITCH - 2 * MARGIN - 2 * half + 1)
}

/// Randomised Prim: grow from room 0 by picking random frontier edges.
fn spanning_tree(rng: &mut Rng, cols: usize, rows: usize) -> Vec<Link> {
    let mut in_tree = vec![false; cols * rows];
    in_tree[0] = true;
    let edges = all_edges(cols, rows);
    let mut links = Vec::new();
    while links.len() + 1 < cols * rows {
        let frontier: Vec<Link> = edges
            .iter()
            .copied()
            .filter(|&(a, b)| in_tree[a] != in_tree[b])
            .collect();
        let pick = rng.below(u32::try_from(frontier.len()).unwrap_or(1));
        let Some(&(a, b)) = frontier.get(pick as usize) else {
            break;
        };
        in_tree[a] = true;
        in_tree[b] = true;
        links.push((a, b));
    }
    links
}

fn all_edges(cols: usize, rows: usize) -> Vec<Link> {
    let mut edges = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            let i = row * cols + col;
            if col + 1 < cols {
                edges.push((i, i + 1));
            }
            if row + 1 < rows {
                edges.push((i, i + cols));
            }
        }
    }
    edges
}

fn add_extra_links(rng: &mut Rng, cols: usize, rows: usize, count: u32, links: &mut Vec<Link>) {
    let edges = all_edges(cols, rows);
    for _ in 0..count {
        // Every grid edge is already a corridor; further picks cannot add
        // one, and `count` is caller-controlled (up to u32::MAX).
        if links.len() >= edges.len() {
            return;
        }
        let pick = rng.below(u32::try_from(edges.len()).unwrap_or(1));
        let Some(&e) = edges.get(pick as usize) else {
            return;
        };
        if !links.contains(&e) {
            links.push(e);
        }
    }
}

/// An L-shaped corridor from the centre of `a` to the centre of `b`. Grid
/// neighbours share a row or column, so each leg crosses only the two rooms
/// and the rock between them. Both ends sit in a room's centre cross, so the
/// cuts overlap the rooms and the open space joins up.
fn corridor(a: &Room, b: &Room, boxes: &mut Boxes) {
    let hw = CORRIDOR_HALF_WIDTH;
    let (x0, x1) = (a.cx.min(b.cx) - hw, a.cx.max(b.cx) + hw);
    let (y0, y1) = (a.cy.min(b.cy) - hw, a.cy.max(b.cy) + hw);
    // Leg along x at a's row, then along y at b's column.
    boxes.push((
        [x0, a.cy - hw, 0],
        [x1, a.cy + hw, CORRIDOR_HEIGHT],
        Op::Subtract,
    ));
    boxes.push((
        [b.cx - hw, y0, 0],
        [b.cx + hw, y1, CORRIDOR_HEIGHT],
        Op::Subtract,
    ));
}

/// A floor-to-ceiling column in one quadrant of the room, clear of the centre
/// cross (corridor mouths) and at least 3 units from every wall. Spanning the
/// full height deliberately makes its faces coplanar with floor and ceiling.
fn pillar(rng: &mut Rng, r: &Room) -> ([i32; 3], [i32; 3], Op) {
    let (px, py) = (offset(rng, r.hx), offset(rng, r.hy));
    let (x, y) = (r.cx + px, r.cy + py);
    (
        [x - PILLAR_HALF, y - PILLAR_HALF, 0],
        [x + PILLAR_HALF, y + PILLAR_HALF, r.height],
        Op::Add,
    )
}

/// Signed offset from the room centre along one axis: magnitude in
/// `4..=half-4`, so the pillar clears the corridor band (half-width 2) and
/// the wall.
fn offset(rng: &mut Rng, half: i32) -> i32 {
    let mag = 4 + below_i32(rng, half - 7);
    if rng.below(2) == 0 { mag } else { -mag }
}
