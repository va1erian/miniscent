//! What a frame draws beyond the level and its actors.

use super::movers::MoverPose;
use crate::dynlight::DynLight;
use crate::particles::{ParticleDraws, SpriteBank};

/// Extras of one frame, all read-only: nothing here is changed by drawing,
/// so every band sees the same and the picture does not depend on the number
/// of workers.
#[derive(Clone, Copy, Default)]
pub struct Effects<'a> {
    /// Particles over the finished scene.
    pub particles: Option<(&'a ParticleDraws, &'a SpriteBank)>,
    /// Lights on the walls and actors they reach. Only the first
    /// [`crate::dynlight::MAX_DYN_LIGHTS`] are used.
    pub lights: &'a [DynLight],
    /// Where each mover is, by its order in the level. A mover with no entry
    /// is drawn as authored.
    pub poses: &'a [MoverPose],
}
