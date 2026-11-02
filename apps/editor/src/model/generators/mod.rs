//! Generators: structure built, not sculpted. Each turns a seed and a place on
//! a surface into a bounded list of primitives, each added or carved, and
//! nothing else. They are pure, so the same seed always gives the same list,
//! and the list goes through the same dab code as a brush stroke, which is why
//! undo, light and limits need nothing of their own. Living things come in their
//! own materials, which is most of what makes them read; drips take the site's.

mod coral;
mod field;
mod hive;
mod mushrooms;
mod site;
mod stalactites;
mod trees;

#[cfg(test)]
mod showcase;
#[cfg(test)]
mod showcase_tests;
#[cfg(test)]
mod tests;

use mscent_math::Vec3d;
use mscent_math::rng::Rng;
use mscent_world::voxel::{DabOp, Prim};

pub use field::{FieldKind, field};
pub use site::{Site, surface_normal};

/// Most primitives a generator may return, so no seed or size can ask for an
/// unbounded amount of work.
pub const MAX_PIECES: usize = 4096;

/// Thinnest radius a piece may have, in cells.
const MIN_RADIUS_CELLS: f64 = 1.5;
/// Pieces thinner than this, in cells, are not smoothed. Each pass shrinks a thin
/// limb, and overlapping passes would part a twig from its branch.
const MIN_SMOOTHED_CELLS: f64 = 3.0;
/// How far past a piece, in cells, its smoothing reaches. A smoothing dab does
/// nothing at its own edge, and it is the piece's surface that needs it.
const SMOOTH_REACH_CELLS: f64 = 2.0;

/// One primitive of a plan and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub prim: Prim,
    pub op: DabOp,
}

/// What a generator returns. Radii near one cell alias into broken beads, so
/// every shape added here is widened to at least [`MIN_RADIUS_CELLS`]. Carves are
/// left as asked: a wider hollow eats the small thing it was hollowing.
#[derive(Clone, Debug)]
pub struct Plan {
    pieces: Vec<Piece>,
    cell: f64,
}

impl Plan {
    pub fn new(cell: f64) -> Self {
        Self {
            pieces: Vec::new(),
            cell,
        }
    }

    pub fn pieces(&self) -> &[Piece] {
        &self.pieces
    }

    pub fn is_full(&self) -> bool {
        self.pieces.len() >= MAX_PIECES
    }

    /// The radius a shape may have.
    pub fn thick(&self, radius: f64) -> f64 {
        radius.max(self.cell * MIN_RADIUS_CELLS)
    }

    fn sized(&self, radius: f64, carve: bool) -> f64 {
        if carve { radius } else { self.thick(radius) }
    }

    /// Softens round what the plan has carved. A prism's hard corners are finer
    /// than the lattice, and meshed as they are they alias into spikes.
    fn smooth_carved(&mut self) {
        let carved: Vec<Prim> = (self.pieces.iter())
            .filter(|p| p.op == DabOp::Carve)
            .map(|p| p.prim)
            .collect();
        let reach = self.cell * SMOOTH_REACH_CELLS;
        for prim in carved {
            self.push(prim.grown(reach), DabOp::Smooth);
        }
    }

    /// Smooths what the plan has added, which takes the hard creases out of
    /// where its pieces join. This is what makes growth read as grown rather
    /// than assembled. What does not fit is left as it is.
    fn smooth_added(&mut self) {
        let thick_enough = self.cell * MIN_SMOOTHED_CELLS;
        let added: Vec<Prim> = self
            .pieces
            .iter()
            .filter(|p| matches!(p.op, DabOp::Add { .. }) && p.prim.thinnest() >= thick_enough)
            .map(|p| p.prim)
            .collect();
        let reach = self.cell * SMOOTH_REACH_CELLS;
        for prim in added {
            self.push(prim.grown(reach), DabOp::Smooth);
        }
    }

    fn push(&mut self, prim: Option<Prim>, op: DabOp) {
        if let (Some(prim), false) = (prim, self.is_full()) {
            self.pieces.push(Piece { prim, op });
        }
    }

    fn op(carve: bool, material: u8) -> DabOp {
        if carve {
            DabOp::Carve
        } else {
            DabOp::Add { material }
        }
    }

    pub fn sphere(&mut self, centre: Vec3d, radius: f64, carve: bool, material: u8) {
        let prim = Prim::sphere(centre, self.sized(radius, carve));
        self.push(prim, Self::op(carve, material));
    }

    pub fn cone(&mut self, a: Vec3d, b: Vec3d, radii: (f64, f64), carve: bool, material: u8) {
        let (ra, rb) = (self.sized(radii.0, carve), self.sized(radii.1, carve));
        self.push(Prim::round_cone(a, b, ra, rb), Self::op(carve, material));
    }

    /// An ellipsoid with `along` and `across` radii about `axis`.
    pub fn ellipsoid(
        &mut self,
        centre: Vec3d,
        axis: Vec3d,
        radii: (f64, f64),
        carve: bool,
        material: u8,
    ) {
        let (along, across) = (self.sized(radii.0, carve), self.sized(radii.1, carve));
        let prim = Prim::spheroid(centre, axis, along, across);
        self.push(prim, Self::op(carve, material));
    }

    pub fn hex(&mut self, centre: Vec3d, axis: Vec3d, corner: Vec3d, size: (f64, f64)) {
        let prim = Prim::hex_prism(centre, axis, corner, self.thick(size.0), self.thick(size.1));
        self.push(prim, DabOp::Carve);
    }

    /// The corners of a box around everything, or `None` for an empty plan.
    /// Smoothing reaches past a piece but makes nothing there, so it does not count.
    pub fn bounds(&self) -> Option<(Vec3d, Vec3d)> {
        let shaped = self.pieces.iter().filter(|p| p.op != DabOp::Smooth);
        let mut boxes = shaped.map(|p| p.prim.bounds());
        let first = boxes.next()?;
        Some(boxes.fold(first, |(lo, hi), (l, h)| (lo.min(l), hi.max(h))))
    }
}

/// The generators offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Stalactites,
    Mushrooms,
    Coral,
    Tree,
    Hive,
    /// A whole ridged-noise range, built as a volume by `mscent_world`. It has no
    /// plan: the other kinds are pieces dabbed into a volume.
    Mountains,
}

impl Kind {
    pub const ALL: [Kind; 6] = [
        Kind::Stalactites,
        Kind::Mushrooms,
        Kind::Coral,
        Kind::Tree,
        Kind::Hive,
        Kind::Mountains,
    ];

    pub const NAMES: [&'static str; 6] = [
        "Stalactites",
        "Mushrooms",
        "Coral",
        "Tree",
        "Hive",
        "Mountains",
    ];

    /// Whether [`generate`] returns pieces for it.
    pub fn planned(self) -> bool {
        self != Kind::Mountains
    }

    pub fn name(self) -> &'static str {
        Self::NAMES[Self::ALL.iter().position(|&k| k == self).unwrap_or(0)]
    }
}

/// The plan for `kind` at `site`. The same arguments always give the same plan.
pub fn generate(kind: Kind, site: &Site, seed: u32) -> Plan {
    let mut rng = Rng::new(u64::from(seed) ^ ((kind as u64 + 1) << 40));
    let mut plan = Plan::new(site.cell);
    let Some(frame) = site.frame() else {
        return plan;
    };
    match kind {
        Kind::Stalactites => stalactites::grow(&mut plan, &frame, site, &mut rng),
        Kind::Mushrooms => mushrooms::grow(&mut plan, &frame, site, &mut rng),
        Kind::Coral => coral::grow(&mut plan, &frame, site, &mut rng),
        Kind::Tree => trees::grow(&mut plan, &frame, site, &mut rng),
        Kind::Hive => hive::grow(&mut plan, &frame, site, &mut rng),
        // Built whole by `Sculptor::raise_mountains`, not from pieces.
        Kind::Mountains => return plan,
    }
    if kind == Kind::Hive {
        plan.smooth_carved();
    } else {
        plan.smooth_added();
    }
    plan
}
