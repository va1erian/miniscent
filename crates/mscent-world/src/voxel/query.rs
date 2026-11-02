//! Questions put to a [`VoxelWorld`] at run time: which chunks lie in a box,
//! which triangles in them, and whether a point is solid.
//!
//! "Solid" is the sign of the nearest surface: behind a triangle, along the
//! normal there, is rock. The triangles are those collision sweeps, and the
//! answer is a pure function of them, so replays agree bit for bit. Far from
//! every surface the chunk's recorded verdict is used instead.

use mscent_math::{Aabb, Vec3, Vec3d};

use super::index::CellIndex;
use super::surface::VoxelVertex;
use super::world::{NO_CHUNK, VolumeGeometry, VoxelChunk, VoxelWorld};

/// How far past its box, in chunks, a volume's surface can reach: the mesh
/// runs two samples out into the ring, and this leaves room to spare.
const REACH_CHUNKS: f32 = 0.25;
/// Rings of neighbour chunks searched for the nearest surface. A chunk with
/// any surface has some within a chunk and a half, so two rings is ample.
const SEARCH_RINGS: i32 = 2;

impl VoxelChunk {
    /// Calls `f` with each full-detail triangle that may touch the box
    /// `lo..=hi`, once each. Triangles the box misses by a wide margin are
    /// never handed over, but near ones can be: the caller's own test is exact.
    pub fn triangles_in(&self, lo: Vec3, hi: Vec3, f: impl FnMut([VoxelVertex; 3])) {
        let mesh = &self.lods[0];
        if mesh.bounds.intersects(&Aabb::new(lo, hi)) {
            self.index.query(mesh, lo, hi, f);
        }
    }

    /// The index over this chunk's triangles.
    pub fn cell_index(&self) -> &CellIndex {
        &self.index
    }
}

impl VolumeGeometry {
    fn extent(&self) -> Vec3 {
        let s = self.chunk_size;
        let n = self.chunks.map(f32::from);
        self.origin + Vec3::new(n[0] * s, n[1] * s, n[2] * s)
    }

    /// The chunk coordinates `lo..=hi` per axis whose chunks may hold a
    /// triangle meeting the box, clamped to the volume. `None` for a box with
    /// a NaN in it or a volume with no usable size.
    fn chunk_range(&self, lo: Vec3, hi: Vec3) -> Option<([i32; 3], [i32; 3])> {
        if !(self.chunk_size > 0.0 && self.chunk_size.is_finite()) {
            return None;
        }
        let (mut first, mut last) = ([0; 3], [0; 3]);
        for a in 0..3 {
            let to_chunk = |v: f32| v - [self.origin.x, self.origin.y, self.origin.z][a];
            let low = to_chunk([lo.x, lo.y, lo.z][a]) / self.chunk_size - REACH_CHUNKS;
            let high = to_chunk([hi.x, hi.y, hi.z][a]) / self.chunk_size + REACH_CHUNKS;
            if low.is_nan() || high.is_nan() {
                return None;
            }
            let top = f32::from(self.chunks[a]) - 1.0;
            first[a] = low.floor().clamp(0.0, top) as i32;
            last[a] = high.floor().clamp(0.0, top) as i32;
        }
        Some((first, last))
    }

    fn chunk_at<'a>(&self, world: &'a VoxelWorld, c: [i32; 3]) -> Option<&'a VoxelChunk> {
        let slot = *self.table.get(self.index(c)?)?;
        if slot == NO_CHUNK {
            return None;
        }
        world.chunks.get(slot as usize).map(|c| &**c)
    }
}

impl VoxelWorld {
    /// Calls `f` with each chunk whose surface may meet the box `lo..=hi`.
    /// Each is visited once.
    pub fn for_each_chunk_in(&self, lo: Vec3, hi: Vec3, mut f: impl FnMut(&VoxelChunk)) {
        for g in &self.volumes {
            let Some((first, last)) = g.chunk_range(lo, hi) else {
                continue;
            };
            for z in first[2]..=last[2] {
                for y in first[1]..=last[1] {
                    for x in first[0]..=last[0] {
                        if let Some(chunk) = g.chunk_at(self, [x, y, z]) {
                            f(chunk);
                        }
                    }
                }
            }
        }
    }

    /// Whether a volume's resolved field makes `p` solid. The BSP's own
    /// verdict is not asked: callers say "BSP solid, or this".
    pub fn is_solid(&self, p: Vec3d) -> bool {
        self.volumes.iter().any(|g| self.solid_in(g, p))
    }

    fn solid_in(&self, g: &VolumeGeometry, p: Vec3d) -> bool {
        let (lo, hi) = (g.origin.to_f64(), g.extent().to_f64());
        let q = [p.x, p.y, p.z];
        let (lo, hi) = ([lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z]);
        if !(0..3).all(|a| q[a] >= lo[a] && q[a] < hi[a]) {
            return false;
        }
        let size = f64::from(g.chunk_size);
        let home = [0, 1, 2].map(|a| ((q[a] - lo[a]) / size).floor() as i32);
        match self.nearest_surface(g, p, home, size) {
            Some(hit) => hit.rock,
            None => g
                .index(home)
                .and_then(|i| g.rock.get(i))
                .copied()
                .unwrap_or(false),
        }
    }

    /// Searches the chunks around `home` ring by ring, nearest first, and stops
    /// as soon as a ring further out cannot hold anything closer.
    fn nearest_surface(
        &self,
        g: &VolumeGeometry,
        p: Vec3d,
        home: [i32; 3],
        size: f64,
    ) -> Option<Hit> {
        let mut best = Hit::NONE;
        for ring in 0..=SEARCH_RINGS {
            if best.d2 <= (f64::from(ring) * size).powi(2) {
                break;
            }
            for dz in -ring..=ring {
                for dy in -ring..=ring {
                    for dx in -ring..=ring {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                            continue;
                        }
                        let c = [home[0] + dx, home[1] + dy, home[2] + dz];
                        if let Some(chunk) = g.chunk_at(self, c) {
                            best.search(chunk, p);
                        }
                    }
                }
            }
        }
        best.d2.is_finite().then_some(best)
    }
}

/// The nearest surface point found so far and which side `p` is on.
struct Hit {
    d2: f64,
    rock: bool,
}

impl Hit {
    const NONE: Self = Self {
        d2: f64::INFINITY,
        rock: false,
    };

    fn search(&mut self, chunk: &VoxelChunk, p: Vec3d) {
        let mesh = &chunk.lods[0];
        if mesh.bounds.is_empty() || distance_squared_to(&mesh.bounds, p) >= self.d2 {
            return;
        }
        let at = [p.x, p.y, p.z];
        chunk.index.nearest(mesh, at, &mut self.d2, |tri, bound| {
            let (q, w) = closest_on_triangle(p, tri.map(|v| v.pos.to_f64()));
            let d2 = (p - q).length_squared();
            if d2 < *bound {
                *bound = d2;
                self.rock = behind(p - q, &tri, w);
            }
        });
    }
}

fn distance_squared_to(b: &Aabb, p: Vec3d) -> f64 {
    let (lo, hi) = (b.min.to_f64(), b.max.to_f64());
    let gap = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
    let (x, y, z) = (
        gap(p.x, lo.x, hi.x),
        gap(p.y, lo.y, hi.y),
        gap(p.z, lo.z, hi.z),
    );
    x * x + y * y + z * z
}

/// Whether `away`, the vector from a surface point to `p`, points into rock:
/// against the surface normal there. At an edge or corner the normal is that
/// of the vertices blended by `w`, so both faces of a ridge agree; the plane's
/// own normal serves where that blend is useless.
fn behind(away: Vec3d, tri: &[VoxelVertex; 3], w: [f64; 3]) -> bool {
    let [a, b, c] = tri.map(|v| v.pos.to_f64());
    let face = (b - a).cross(c - a);
    let blend = (0..3).fold(Vec3d::ZERO, |sum, i| sum + tri[i].normal.to_f64() * w[i]);
    let normal = if blend.dot(face) > 0.0 { blend } else { face };
    away.dot(normal) <= 0.0
}

/// Point of the triangle `t` nearest to `p`, with its barycentric weights.
/// After Ericson, Real-Time Collision Detection 5.1.5.
fn closest_on_triangle(p: Vec3d, t: [Vec3d; 3]) -> (Vec3d, [f64; 3]) {
    let [a, b, c] = t;
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, [1.0 - v, v, 0.0]);
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let v = d2 / (d2 - d6);
        return (a + ac * v, [1.0 - v, 0.0, v]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        let v = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * v, [0.0, 1.0 - v, v]);
    }
    let sum = va + vb + vc;
    if sum == 0.0 || !sum.is_finite() {
        return (a, [1.0, 0.0, 0.0]);
    }
    let (v, w) = (vb / sum, vc / sum);
    (a + ab * v + ac * w, [1.0 - v - w, v, w])
}
