//! Enemy models from blueprints: hard pieces from brushes and soft ones from
//! voxels, in one mesh (design in `docs/enemies.md`).
//!
//! A [`Blueprint`] is a small text file listing the pieces of a design. It is
//! read as untrusted input, and everything in it is checked by [`read`], so a
//! [`Blueprint`] value is always one that [`estimated_ns`] can judge and
//! [`build`] can attempt. Positions are in model units, which [`Blueprint::scale`]
//! turns into world units.
//!
//! * `shape.rs`: the shapes a piece can have, as brushes and as [`Prim`]s.
//! * `read.rs`, `read_shape.rs`, `write.rs`: the text form.
//! * `cost.rs`: what a build will take, worked out before any of it is done.
//! * `build.rs`: pieces to one mesh per part at three levels of detail.
//!
//! [`Prim`]: crate::voxel::Prim

mod build;
mod cost;
mod read;
mod read_shape;
mod shape;
mod write;

#[cfg(test)]
mod build_tests;
#[cfg(test)]
mod hostile_tests;
#[cfg(test)]
mod read_tests;

use core::fmt;

use mscent_math::Vec3d;

use crate::brush::BrushError;
use crate::csg::CsgError;
use crate::voxel::{CellSize, VolumeError};

pub use build::{LEVELS, Model, ModelPart, PartMesh, PartVertex, build};
pub use cost::{MAX_MODEL_NS, estimated_ns};
pub use read::{MAX_FILE, read};
pub use shape::{Hard, MAX_NGON_SIDES, MAX_PLATE_POINTS, Soft};
pub use write::write;

/// Most parts in a model. Each adds a matrix to every frame it is drawn in.
pub const MAX_PARTS: usize = 16;
/// Most pieces in a model, over all its parts.
pub const MAX_PIECES: usize = 512;
/// Most solids and cuts a part with cuts may hold: CSG grows faster than they do.
pub const MAX_CUT_BRUSHES: usize = 64;
/// Most chunks in the boxes of a model's voxel parts, empty ones included.
pub const MAX_MODEL_CHUNKS: usize = 256;
/// Most hit spheres in a model.
pub const MAX_HITS: usize = 8;
/// Most material roles in a model.
pub const MAX_ROLES: usize = 16;
/// Longest name, in bytes, of a design, part, role or material.
pub const MAX_NAME: usize = 32;
/// Largest coordinate or size in a blueprint, in model units.
pub const MAX_MODEL_COORD: f64 = 256.0;
/// Largest world units per model unit.
pub const MAX_SCALE: f64 = 16.0;
/// Scale when a blueprint does not give one: a quarter puts the brushes' snap
/// grid at 1/32 of a world unit.
pub const DEFAULT_SCALE: f64 = 0.25;

/// What sort of enemy a design is, which sets its budgets in the foundry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Drone,
    Ship,
    Alien,
}

/// How a part moves about its pivot, with its rate in cycles a second. Drawing
/// alone reads this; the simulation never does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Motion {
    Still,
    /// Turns about the axis without end.
    Spin {
        rate: f64,
    },
    /// Swings about the axis by up to `angle` radians each way.
    Swing {
        angle: f64,
        rate: f64,
    },
    /// Waves by up to `reach` model units, more the farther from the pivot.
    Sway {
        reach: f64,
        rate: f64,
    },
}

/// A name for a material: what a role is made of unless a kind says otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skin {
    pub role: String,
    pub material: String,
    /// Drawn flat, with no light, so it stays bright in a dark room.
    pub glow: bool,
}

/// One step of a part. `role` indexes [`Blueprint::skins`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Piece {
    /// A hard shape. `detail` leaves it out below full detail, unless a cut
    /// touches it, in which case its faces cannot be told from the others'.
    Solid { shape: Hard, role: u8, detail: bool },
    /// Takes a hard shape out of the solids and the voxels of its part. `role`
    /// is what the walls it leaves on solids are made of.
    Cut { shape: Hard, role: u8 },
    /// Adds a soft shape.
    Add { shape: Soft, role: u8 },
    /// Removes a soft shape from the voxels added so far.
    Carve { shape: Soft },
    /// Smooths the voxels within a soft shape.
    Smooth { shape: Soft },
    /// Changes the material of the voxels within a soft shape.
    Paint { shape: Soft, role: u8 },
}

/// A rigid piece of a model, built alone so it can move alone.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    /// Cell of its voxels. Thin limbs need a finer one than a body.
    pub cell: CellSize,
    pub pivot: Vec3d,
    /// Unit length.
    pub axis: Vec3d,
    pub motion: Motion,
    pub pieces: Vec<Piece>,
}

/// A sphere the simulation hits. Written in the file, never worked out at load,
/// so replay does not depend on any mesh.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub centre: Vec3d,
    pub radius: f64,
}

/// A design, as read from a file.
#[derive(Clone, Debug, PartialEq)]
pub struct Blueprint {
    pub name: String,
    pub class: Class,
    /// The body plan and seed that made it, if one did.
    pub plan: Option<(String, u64)>,
    pub scale: f64,
    pub skins: Vec<Skin>,
    pub parts: Vec<Part>,
    pub hits: Vec<Hit>,
}

/// Why a model could not be read or built.
#[derive(Clone, Debug, PartialEq)]
pub enum ModelError {
    /// A file could not be read. Line 0 is the file as a whole.
    Parse {
        line: usize,
        why: String,
    },
    /// Building would take more than [`MAX_MODEL_NS`], by [`estimated_ns`].
    TooCostly {
        ns: u64,
    },
    /// A part's mesh has more vertices than a `u16` index reaches.
    TooManyVertices,
    Brush(BrushError),
    Csg(CsgError),
    Volume(VolumeError),
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse { line, why } => write!(f, "line {line}: {why}"),
            Self::TooCostly { ns } => write!(
                f,
                "the model would take about {} ms to build, and the limit is {} ms",
                ns / 1_000_000,
                MAX_MODEL_NS / 1_000_000
            ),
            Self::TooManyVertices => write!(f, "a part has too many vertices"),
            Self::Brush(e) => write!(f, "bad brush: {e:?}"),
            Self::Csg(e) => write!(f, "cannot cut the solids: {e:?}"),
            Self::Volume(e) => write!(f, "bad volume: {e:?}"),
        }
    }
}

impl std::error::Error for ModelError {}
