//! What the brushes contribute to one volume: `base`, the later cuts and the
//! world just outside the box.

use mscent_math::Vec3d;
use mscent_math::epsilon::VOXEL_SEAM_BIAS_CELLS;

use super::chunk::sample_index;
use super::volume::{Lattice, Volume};
use super::{
    CHUNK_EDGE, CHUNK_SAMPLES, FALLBACK_MATERIAL, INHERIT_MATERIAL, MAX_SAMPLE, UNITS_PER_CELL,
    edited, quantize,
};
use crate::brush::{Brush, Op};

/// Cells past its bounds where a brush stops counting. Past the sample range
/// of two cells it cannot change a value, and the margin absorbs the `f32`
/// rounding of [`Brush::bounds`].
const REACH_CELLS: f64 = 3.0;

/// A brush with the bounds of the samples it can reach. The sample-level test
/// makes a value independent of which chunk asks for it: chunks that share a
/// padding sample must compute the same number or the mesh cracks.
pub(super) struct Reach<'a> {
    brush: &'a Brush,
    lo: Vec3d,
    hi: Vec3d,
}

impl<'a> Reach<'a> {
    fn new(brush: &'a Brush, cell: f64) -> Self {
        let b = brush.bounds();
        let margin = REACH_CELLS * cell;
        Self {
            brush,
            lo: b.min.to_f64() - Vec3d::new(margin, margin, margin),
            hi: b.max.to_f64() + Vec3d::new(margin, margin, margin),
        }
    }

    fn holds(&self, p: Vec3d) -> bool {
        p.x >= self.lo.x
            && p.x <= self.hi.x
            && p.y >= self.lo.y
            && p.y <= self.hi.y
            && p.z >= self.lo.z
            && p.z <= self.hi.z
    }

    fn overlaps(&self, lo: Vec3d, hi: Vec3d) -> bool {
        lo.x <= self.hi.x
            && hi.x >= self.lo.x
            && lo.y <= self.hi.y
            && hi.y >= self.lo.y
            && lo.z <= self.hi.z
            && hi.z >= self.lo.z
    }

    /// Distance inside the brush, negative outside. It is the nearest plane's
    /// distance, whose sign is exact for a convex solid.
    fn depth(&self, p: Vec3d) -> f64 {
        let outside = self
            .brush
            .planes()
            .iter()
            .fold(f64::NEG_INFINITY, |d, pl| d.max(pl.signed_distance(p)));
        -outside
    }

    /// [`Self::depth`], with the material of the face that nearest plane holds.
    fn depth_and_material(&self, p: Vec3d) -> (f64, u8) {
        let mut outside = f64::NEG_INFINITY;
        let mut material = self.brush.material();
        let faces = self.brush.planes().iter().zip(self.brush.texinfos());
        for (plane, info) in faces {
            let d = plane.signed_distance(p);
            if d > outside {
                outside = d;
                material = info.material;
            }
        }
        (-outside, sample_material(material))
    }

    /// Squared distance from `p` to the brush's bounds.
    fn distance2(&self, p: Vec3d) -> f64 {
        let gap = |v: f64, lo: f64, hi: f64| (lo - v).max(v - hi).max(0.0);
        let d = Vec3d::new(
            gap(p.x, self.lo.x, self.hi.x),
            gap(p.y, self.lo.y, self.hi.y),
            gap(p.z, self.lo.z, self.hi.z),
        );
        d.dot(d)
    }
}

/// A brush's material as a sample holds it.
fn sample_material(material: u16) -> u8 {
    u8::try_from(material)
        .ok()
        .filter(|&m| m != INHERIT_MATERIAL)
        .unwrap_or(FALLBACK_MATERIAL)
}

/// Applies brushes in order over `start`. Positive is solid.
fn fold<'r, 'a: 'r>(brushes: impl Iterator<Item = &'r Reach<'a>>, start: f64, p: Vec3d) -> f64 {
    let mut d = start;
    for r in brushes {
        if !r.holds(p) {
            continue;
        }
        let depth = r.depth(p);
        d = match r.brush.op() {
            Op::Add => d.max(depth),
            Op::Subtract => d.min(-depth),
        };
    }
    d
}

/// [`fold`] from solid, with the material of the brush that decided the result,
/// or `fallback` if none reaches `p`.
fn fold_material(brushes: &[&Reach], fallback: u8, p: Vec3d) -> (f64, u8) {
    let (mut d, mut material) = (f64::INFINITY, fallback);
    for r in brushes {
        if !r.holds(p) {
            continue;
        }
        let (depth, m) = r.depth_and_material(p);
        let next = match r.brush.op() {
            Op::Add => d.max(depth),
            Op::Subtract => d.min(-depth),
        };
        if next != d {
            (d, material) = (next, m);
        }
    }
    (d, material)
}

/// The `base` samples of a chunk.
pub(super) enum BaseChunk {
    Uniform(i8, u8),
    Detail(Box<BaseSamples>),
}

pub(super) struct BaseSamples {
    value: [i8; CHUNK_SAMPLES],
    material: [u8; CHUNK_SAMPLES],
}

impl BaseChunk {
    pub fn get(&self, i: usize) -> i8 {
        match self {
            BaseChunk::Uniform(v, _) => *v,
            BaseChunk::Detail(d) => d.value[i],
        }
    }

    /// What a sample storing `stored` shows at `i`.
    pub fn material(&self, i: usize, stored: u8) -> u8 {
        if stored != INHERIT_MATERIAL {
            return stored;
        }
        match self {
            BaseChunk::Uniform(_, m) => *m,
            BaseChunk::Detail(d) => d.material[i],
        }
    }
}

/// A volume touching this one, read across the shared border.
struct Neighbour<'a> {
    volume: &'a Volume,
    ctx: Context<'a>,
}

impl Neighbour<'_> {
    /// Its resolved sample nearest `p` and that sample's material, if `p` is
    /// inside it. Lattices of one cell size line up, so that is the sample at
    /// `p`; another size gives a surface that only overlaps, not one that meets.
    fn sample(&mut self, p: Vec3d, cell: f64) -> Option<(i8, u8)> {
        let lattice = self.volume.lattice;
        let own = lattice.cell.units();
        let at = |v: f64, axis: usize| (v / own).floor() as i32 - lattice.origin[axis];
        let g = [at(p.x, 0), at(p.y, 1), at(p.z, 2)];
        let samples = lattice.samples();
        if (0..3).any(|a| g[a] < 0 || g[a] >= samples[a]) {
            return None;
        }
        let edge = CHUNK_EDGE as i32;
        let coord = g.map(|v| v.div_euclid(edge));
        let index = lattice.chunk_index(coord)?;
        let local = g.map(|v| v.rem_euclid(edge) as usize);
        let i = sample_index(local[0], local[1], local[2]);
        self.ctx.ensure_base(index, coord);
        let base = self.ctx.cached_base(index)?;
        let (delta, stored) = self.volume.table.get(index)?.sample(i);
        let material = base.material(i, stored);
        let value = edited(base.get(i), delta);
        let cut = fold(self.ctx.cuts.iter(), f64::INFINITY, lattice.position(g));
        let value = if cut == f64::INFINITY {
            value
        } else {
            value.min(quantize(cut, own))
        };
        if own == cell {
            return Some((value, material));
        }
        let distance = f64::from(value) / UNITS_PER_CELL * own;
        Some((quantize(distance, cell), material))
    }
}

/// The brushes as one volume sees them, with `base` cached per chunk. Build a
/// new one when the brushes or the volume's place in the world change.
pub struct Context<'a> {
    pub(super) lattice: Lattice,
    before: Vec<Reach<'a>>,
    cuts: Vec<Reach<'a>>,
    all: Vec<Reach<'a>>,
    base: Vec<Option<BaseChunk>>,
    neighbours: Vec<Neighbour<'a>>,
}

impl<'a> Context<'a> {
    pub fn new(volume: &Volume, brushes: &'a [Brush]) -> Self {
        let lattice = volume.lattice;
        let cell = lattice.cell.units();
        let mut sorted: Vec<&Brush> = brushes.iter().collect();
        sorted.sort_by_key(|b| b.order());
        let reach = |b: &&'a Brush| Reach::new(b, cell);
        let order = lattice.order;
        Self {
            lattice,
            before: sorted
                .iter()
                .filter(|b| b.order() < order)
                .map(reach)
                .collect(),
            cuts: sorted
                .iter()
                .filter(|b| b.order() > order && b.op() == Op::Subtract)
                .map(reach)
                .collect(),
            all: sorted.iter().map(reach).collect(),
            base: std::iter::repeat_with(|| None)
                .take(lattice.chunk_count())
                .collect(),
            neighbours: Vec::new(),
        }
    }

    /// [`Self::new`], for a volume among the `volumes` of its level: a tunnel
    /// crossing into a volume that touches this one then has no wall in it.
    pub fn in_level(volume: &Volume, brushes: &'a [Brush], volumes: &'a [Volume]) -> Self {
        let mut ctx = Self::new(volume, brushes);
        ctx.neighbours = volumes
            .iter()
            .filter(|other| other.lattice != volume.lattice && reaches(volume, other))
            .map(|other| Neighbour {
                volume: other,
                ctx: Context::new(other, brushes),
            })
            .collect();
        ctx
    }

    pub(super) fn has_neighbours(&self) -> bool {
        !self.neighbours.is_empty()
    }

    /// What a touching volume says of ring sample `p`, if one holds it.
    pub(super) fn neighbour_sample(&mut self, p: Vec3d) -> Option<(i8, u8, Op)> {
        let cell = self.lattice.cell.units();
        self.neighbours.iter_mut().find_map(|n| {
            let op = n.volume.op();
            n.sample(p, cell)
                .map(|(value, material)| (value, material, op))
        })
    }

    /// Computes the `base` of chunk `index` at `coord` unless it is cached.
    pub(super) fn ensure_base(&mut self, index: usize, coord: [i32; 3]) {
        if self.base.get(index).is_some_and(Option::is_none) {
            self.base[index] = Some(self.compute_base(coord));
        }
    }

    /// The `base` of chunk `index`, after [`Self::ensure_base`].
    pub(super) fn cached_base(&self, index: usize) -> Option<&BaseChunk> {
        self.base.get(index)?.as_ref()
    }

    fn compute_base(&self, coord: [i32; 3]) -> BaseChunk {
        if self.lattice.op == Op::Add {
            return BaseChunk::Uniform(-MAX_SAMPLE, 0);
        }
        let edge = CHUNK_EDGE as i32;
        let first = coord.map(|c| c * edge);
        let (lo, hi) = (
            self.lattice.position(first),
            self.lattice.position(first.map(|g| g + edge - 1)),
        );
        let deep = self.nearest_material((lo + hi) * 0.5);
        let list = near(&self.before, lo, hi);
        if list.is_empty() {
            return BaseChunk::Uniform(MAX_SAMPLE, deep);
        }
        let cell = self.lattice.cell.units();
        let mut out = Box::new(BaseSamples {
            value: [0; CHUNK_SAMPLES],
            material: [0; CHUNK_SAMPLES],
        });
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let g = [
                        first[0] + x as i32,
                        first[1] + y as i32,
                        first[2] + z as i32,
                    ];
                    let (value, material) = fold_material(&list, deep, self.lattice.position(g));
                    let i = sample_index(x, y, z);
                    out.value[i] = quantize(value, cell);
                    out.material[i] = material;
                }
            }
        }
        BaseChunk::Detail(out)
    }

    /// What rock too deep for any brush to decide looks like: the brush nearest
    /// `p`, so a long tunnel keeps the look of the wall it started from.
    fn nearest_material(&self, p: Vec3d) -> u8 {
        self.before
            .iter()
            .min_by(|a, b| a.distance2(p).total_cmp(&b.distance2(p)))
            .map_or(0, |r| sample_material(r.brush.material()))
    }

    /// The subtract brushes after the volume that can reach samples between
    /// `lo` and `hi`.
    pub(super) fn cuts_near(&self, lo: Vec3d, hi: Vec3d) -> Vec<&Reach<'a>> {
        near(&self.cuts, lo, hi)
    }

    /// Every brush that can reach samples between `lo` and `hi`.
    pub(super) fn world_near(&self, lo: Vec3d, hi: Vec3d) -> Vec<&Reach<'a>> {
        near(&self.all, lo, hi)
    }

    /// A ring sample at `p`: a subtract volume's shows the brushes outside the
    /// box, moved 1/16 cell into the solid so the voxel surface slides under
    /// the brush faces it meets. An add volume ends in air.
    pub(super) fn ring_sample(&self, near_world: &[&Reach], p: Vec3d) -> i8 {
        if self.lattice.op == Op::Add {
            return -MAX_SAMPLE;
        }
        let cell = self.lattice.cell.units();
        let world = fold(near_world.iter().copied(), f64::INFINITY, p);
        let biased = world - VOXEL_SEAM_BIAS_CELLS * cell;
        quantize(biased, cell)
    }
}

/// `edited` cut by later subtract brushes.
pub(super) fn apply_cuts(cuts: &[&Reach], edited: i8, p: Vec3d, cell: f64) -> i8 {
    if cuts.is_empty() {
        return edited;
    }
    let cut = fold(cuts.iter().copied(), f64::INFINITY, p);
    if cut == f64::INFINITY {
        return edited;
    }
    edited.min(quantize(cut, cell))
}

/// Whether `other` holds samples of `volume`'s ring: its box within two cells.
pub(super) fn reaches(volume: &Volume, other: &Volume) -> bool {
    let ring = 2.0 * volume.lattice.cell.units();
    let ((a0, a1), (b0, b1)) = (volume.bounds(), other.bounds());
    a0.x - ring < b1.x
        && b0.x < a1.x + ring
        && a0.y - ring < b1.y
        && b0.y < a1.y + ring
        && a0.z - ring < b1.z
        && b0.z < a1.z + ring
}

fn near<'r, 'a>(list: &'r [Reach<'a>], lo: Vec3d, hi: Vec3d) -> Vec<&'r Reach<'a>> {
    list.iter().filter(|r| r.overlaps(lo, hi)).collect()
}
