//! The player's ship: a model that follows the player's pose, for the GPU
//! renderer's shadows and for a chase view. It is cosmetic. It is made from the
//! frame's own interpolated pose and never read by the simulation, so collision,
//! aim and replays do not see it. Software, which has no shadows, never draws it,
//! chase view or not: the session hands software a frame without models
//! (`software_input`). The ship's code and blueprint are only in a build with
//! the hardware renderer; the rest of what this work added to the software build
//! is in `docs/hardware-effects.md`.
//!
//! The blueprint is authored in the player's frame (nose along -Z, up along +Y),
//! so the player's orientation turns it as it is. It is drawn from the level's
//! model table, beside the enemies, so a frame has one table.

use mscent_math::{Quat, Vec3};
use mscent_render::LevelRenderer;
use mscent_render::model::{ModelId, ModelInstance};

pub struct Ship {
    #[cfg(feature = "hardware")]
    design: (mscent_world::model::Blueprint, mscent_world::model::Model),
    id: ModelId,
}

impl Ship {
    /// The ship, added to `renderer`'s models with its skins from the level's
    /// materials. `None` in a build without the hardware renderer, and for a
    /// blueprint that does not read, which the tests hold it to.
    #[cfg(feature = "hardware")]
    pub fn build(renderer: &mut LevelRenderer) -> Option<Self> {
        const BLUEPRINT: &str = include_str!("../../../assets/ship/player.msb");
        let blueprint = mscent_world::model::read(BLUEPRINT.as_bytes()).ok()?;
        let model = mscent_world::model::build(&blueprint).ok()?;
        let id = renderer.add_model(&blueprint, &model);
        Some(Self {
            design: (blueprint, model),
            id,
        })
    }

    #[cfg(not(feature = "hardware"))]
    pub fn build(_: &mut LevelRenderer) -> Option<Self> {
        None
    }

    /// Adds the ship to `renderer`, which replaces the one it was added to.
    #[cfg(feature = "hardware")]
    pub fn add_to(&mut self, renderer: &mut LevelRenderer) {
        self.id = renderer.add_model(&self.design.0, &self.design.1);
    }

    #[cfg(not(feature = "hardware"))]
    pub fn add_to(&mut self, _: &mut LevelRenderer) {}

    /// The ship at `pos` turned as `orientation`. In first person it is only a
    /// shadow's caster (`drawn` false): drawn, it would fill the view.
    pub fn instance(&self, pos: Vec3, orientation: Quat, drawn: bool) -> ModelInstance {
        ModelInstance {
            cast_only: !drawn,
            ..ModelInstance::new(self.id, pos, orientation)
        }
    }
}
