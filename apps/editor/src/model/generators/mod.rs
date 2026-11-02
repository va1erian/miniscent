//! Generators: structure built, not sculpted. Each turns a seed and a place on
//! a surface into a bounded list of primitives, each added or carved, and
//! nothing else. They are pure, so the same seed always gives the same list,
//! and the list goes through the same dab code as a brush stroke, which is why
//! undo, light and limits need nothing of their own.

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

/// One primitive of a plan and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub prim: Prim,
    pub op: DabOp,
}

/// What a generator returns. Radii below one cell alias into broken beads, so
/// every shape added here is widened to one.
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

    /// The radius a shape may have: at least one cell.
    pub fn thick(&self, radius: f64) -> f64 {
        radius.max(self.cell)
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
        let prim = Prim::sphere(centre, self.thick(radius));
        self.push(prim, Self::op(carve, material));
    }

    pub fn cone(&mut self, a: Vec3d, b: Vec3d, radii: (f64, f64), carve: bool, material: u8) {
        let (ra, rb) = (self.thick(radii.0), self.thick(radii.1));
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
        let (along, across) = (self.thick(radii.0), self.thick(radii.1));
        let prim = Prim::spheroid(centre, axis, along, across);
        self.push(prim, Self::op(carve, material));
    }

    pub fn hex(&mut self, centre: Vec3d, axis: Vec3d, corner: Vec3d, size: (f64, f64)) {
        let prim = Prim::hex_prism(centre, axis, corner, self.thick(size.0), self.thick(size.1));
        self.push(prim, DabOp::Carve);
    }

    /// The corners of a box around everything, or `None` for an empty plan.
    pub fn bounds(&self) -> Option<(Vec3d, Vec3d)> {
        let mut boxes = self.pieces.iter().map(|p| p.prim.bounds());
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
}

impl Kind {
    pub const ALL: [Kind; 5] = [
        Kind::Stalactites,
        Kind::Mushrooms,
        Kind::Coral,
        Kind::Tree,
        Kind::Hive,
    ];

    pub const NAMES: [&'static str; 5] = ["Stalactites", "Mushrooms", "Coral", "Tree", "Hive"];

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
    }
    plan
}
