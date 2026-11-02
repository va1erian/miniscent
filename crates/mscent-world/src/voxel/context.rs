//! What the brushes contribute to one volume: `base`, the later cuts and the
//! world just outside the box.

use mscent_math::Vec3d;
use mscent_math::epsilon::VOXEL_SEAM_BIAS_CELLS;

use super::chunk::sample_index;
use super::volume::{Lattice, Volume};
use super::{CHUNK_EDGE, CHUNK_SAMPLES, MAX_SAMPLE, quantize};
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
}

/// Applies brushes in order over `start`. Positive is solid.
fn fold(brushes: &[&Reach], start: f64, p: Vec3d) -> f64 {
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

/// The `base` samples of a chunk.
pub(super) enum BaseChunk {
    Uniform(i8),
    Detail(Box<[i8; CHUNK_SAMPLES]>),
}

impl BaseChunk {
    pub fn get(&self, i: usize) -> i8 {
        match self {
            BaseChunk::Uniform(v) => *v,
            BaseChunk::Detail(d) => d[i],
        }
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
        }
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
            return BaseChunk::Uniform(-MAX_SAMPLE);
        }
        let edge = CHUNK_EDGE as i32;
        let first = coord.map(|c| c * edge);
        let list = near(
            &self.before,
            self.lattice.position(first),
            self.lattice.position(first.map(|g| g + edge - 1)),
        );
        if list.is_empty() {
            return BaseChunk::Uniform(MAX_SAMPLE);
        }
        let cell = self.lattice.cell.units();
        let mut out = Box::new([0i8; CHUNK_SAMPLES]);
        for z in 0..CHUNK_EDGE {
            for y in 0..CHUNK_EDGE {
                for x in 0..CHUNK_EDGE {
                    let g = [
                        first[0] + x as i32,
                        first[1] + y as i32,
                        first[2] + z as i32,
                    ];
                    let p = self.lattice.position(g);
                    out[sample_index(x, y, z)] = quantize(fold(&list, f64::INFINITY, p), cell);
                }
            }
        }
        BaseChunk::Detail(out)
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
        let biased = fold(near_world, f64::INFINITY, p) - VOXEL_SEAM_BIAS_CELLS * cell;
        quantize(biased, cell)
    }
}

/// `edited` cut by later subtract brushes.
pub(super) fn apply_cuts(cuts: &[&Reach], edited: i8, p: Vec3d, cell: f64) -> i8 {
    if cuts.is_empty() {
        return edited;
    }
    let cut = fold(cuts, f64::INFINITY, p);
    if cut == f64::INFINITY {
        return edited;
    }
    edited.min(quantize(cut, cell))
}

fn near<'r, 'a>(list: &'r [Reach<'a>], lo: Vec3d, hi: Vec3d) -> Vec<&'r Reach<'a>> {
    list.iter().filter(|r| r.overlaps(lo, hi)).collect()
}
