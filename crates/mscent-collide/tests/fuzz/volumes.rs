//! Voxel volumes for the fuzzer, and an oracle for them that shares no code
//! with the collision under test: no cell index, no contact maths. It measures
//! the distance from a point to the drawn triangles by brute force over a plain
//! hash grid.

use std::collections::HashMap;

use mscent_math::Vec3d;
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, VoxelWorld, apply_dab};
use mscent_world::{Brush, Bsp, Level, Op, compile};

/// Edge of the oracle's hash cells.
const GRID: f64 = 2.0;
/// A point farther than this from every triangle is taken as clear: the fuzzer
/// moves in steps far shorter than this, so it cannot have got past a surface
/// unnoticed.
const NEAR: f64 = 0.06;

/// A generated level with a carved `Subtract` volume in the east wall of its
/// first room and an `Add` volume of growths standing in the room. The two
/// boxes touch at most.
pub struct Volumes {
    /// Brushes and volume shells, which is what the BSP compiled from.
    pub csg: Vec<Brush>,
    pub bsp: Bsp,
    pub world: VoxelWorld,
}

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn sphere(c: Vec3d, r: f64) -> Prim {
    Prim::sphere(c, r).expect("valid")
}

/// Adds the volumes to the generated `brushes`. `None` when the level has no
/// room to put them in.
pub fn add_volumes(brushes: &[Brush]) -> Option<Volumes> {
    let room = brushes.first()?.bounds();
    let (lo, hi) = (room.min.to_f64(), room.max.to_f64());
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let n = brushes.len() as u32;
    // Chunks of 8 units at a cell of a half; origins in cells.
    let cells = |u: f64| (u * 2.0).round() as i32;

    let mut rock = Volume::new(
        Op::Subtract,
        n,
        CellSize::Half,
        [cells(hi.x - 4.0), cells(cy - 8.0), 0],
        [2, 2, 1],
    )
    .ok()?;
    let mut ctx = Context::new(&rock, brushes);
    // A tunnel leaving the wall, a pocket at its end, and a bump on its floor.
    let tunnel = Prim::round_cone(v(hi.x - 1.0, cy, 2.0), v(hi.x + 5.0, cy, 2.0), 1.6, 1.2)?;
    apply_dab(&mut rock, &mut ctx, &tunnel, DabOp::Carve);
    apply_dab(
        &mut rock,
        &mut ctx,
        &sphere(v(hi.x + 5.0, cy, 2.5), 2.4),
        DabOp::Carve,
    );
    let bump = sphere(v(hi.x + 2.0, cy, 0.4), 1.0);
    apply_dab(&mut rock, &mut ctx, &bump, DabOp::Add { material: 1 });

    let mut growths = Volume::new(
        Op::Add,
        n + 1,
        CellSize::Half,
        [cells(cx - 4.0), cells(cy - 4.0), 0],
        [1, 1, 1],
    )
    .ok()?;
    let mut ctx = Context::new(&growths, brushes);
    let mound = sphere(v(cx, cy, 0.5), 2.0);
    apply_dab(&mut growths, &mut ctx, &mound, DabOp::Add { material: 1 });
    let spike = Prim::round_cone(v(cx + 4.0, cy, 0.0), v(cx + 4.0, cy, 5.0), 1.5, 0.3)?;
    apply_dab(&mut growths, &mut ctx, &spike, DabOp::Add { material: 2 });

    let level = Level {
        brushes: brushes.to_vec(),
        volumes: vec![rock, growths],
        ..Default::default()
    };
    let csg = level.csg_brushes().into_owned();
    let bsp = compile(&csg).ok()?;
    let world = VoxelWorld::build(&level, &level.static_brushes(), &bsp);
    Some(Volumes { csg, bsp, world })
}

type Tri = [Vec3d; 3];

/// Distance and side from a point to the drawn triangles, found by brute force.
pub struct VoxelOracle {
    tris: Vec<(Tri, [Vec3d; 3])>,
    cells: HashMap<[i32; 3], Vec<u32>>,
}

fn cell_of(p: Vec3d) -> [i32; 3] {
    [p.x, p.y, p.z].map(|c| (c / GRID).floor() as i32)
}

impl VoxelOracle {
    pub fn new(world: &VoxelWorld) -> Self {
        let mut tris = Vec::new();
        for chunk in &world.chunks {
            let mesh = &chunk.lods[0];
            for t in &mesh.triangles {
                let vs = t.map(|i| mesh.vertices[usize::from(i)]);
                tris.push((vs.map(|x| x.pos.to_f64()), vs.map(|x| x.normal.to_f64())));
            }
        }
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (i, (t, _)) in tris.iter().enumerate() {
            let lo = cell_of(t[0].min(t[1]).min(t[2]));
            let hi = cell_of(t[0].max(t[1]).max(t[2]));
            for z in lo[2]..=hi[2] {
                for y in lo[1]..=hi[1] {
                    for x in lo[0]..=hi[0] {
                        cells.entry([x, y, z]).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { tris, cells }
    }

    /// The nearest triangle point within `reach` of `p`, as its distance and
    /// whether `p` is behind the surface there.
    fn nearest(&self, p: Vec3d, reach: f64) -> Option<(f64, bool)> {
        let pad = v(reach, reach, reach);
        let (lo, hi) = (cell_of(p - pad), cell_of(p + pad));
        let mut best: Option<(f64, bool)> = None;
        for z in lo[2]..=hi[2] {
            for y in lo[1]..=hi[1] {
                for x in lo[0]..=hi[0] {
                    for &i in self.cells.get(&[x, y, z]).into_iter().flatten() {
                        let (tri, normals) = &self.tris[i as usize];
                        let (q, w) = closest(p, tri);
                        let d = (p - q).length();
                        if d <= reach && best.is_none_or(|b| d < b.0) {
                            let n = (0..3).fold(Vec3d::ZERO, |s, k| s + normals[k] * w[k]);
                            best = Some((d, (p - q).dot(n) <= 0.0));
                        }
                    }
                }
            }
        }
        best
    }

    /// True if the surface has a point within `r - tolerance` of `p`, or `p` is
    /// behind it, when that is judged near enough to be sure.
    pub fn overlaps(&self, p: Vec3d, r: f64, tolerance: f64) -> bool {
        match self.nearest(p, r.max(NEAR) + 0.5) {
            Some((d, behind)) => d < r - tolerance || (behind && d < NEAR),
            None => false,
        }
    }
}

/// Nearest point of the triangle to `p` and its barycentric weights, by
/// comparing the face, three edges and three corners rather than by regions.
fn closest(p: Vec3d, t: &Tri) -> (Vec3d, [f64; 3]) {
    let [a, b, c] = *t;
    let mut best = (a, [1.0, 0.0, 0.0]);
    let mut best_d = (p - a).length();
    let mut offer = |q: Vec3d, w: [f64; 3]| {
        let d = (p - q).length();
        if d < best_d {
            (best, best_d) = ((q, w), d);
        }
    };
    offer(b, [0.0, 1.0, 0.0]);
    offer(c, [0.0, 0.0, 1.0]);
    for (i, j) in [(0, 1), (1, 2), (2, 0)] {
        let (s, e) = (t[i], t[j]);
        let ab = e - s;
        let len2 = ab.dot(ab);
        if len2 > 0.0 {
            let u = ((p - s).dot(ab) / len2).clamp(0.0, 1.0);
            let mut w = [0.0; 3];
            (w[i], w[j]) = (1.0 - u, u);
            offer(s + ab * u, w);
        }
    }
    let n = (b - a).cross(c - a);
    let area2 = n.dot(n);
    if area2 > 0.0 {
        let q = p - n * ((p - a).dot(n) / area2);
        let w = [
            (c - b).cross(q - b).dot(n) / area2,
            (a - c).cross(q - c).dot(n) / area2,
            (b - a).cross(q - a).dot(n) / area2,
        ];
        if w.iter().all(|&x| x >= 0.0) {
            offer(q, w);
        }
    }
    best
}
