//! The player's ship and the pilot as models that follow the game's poses, for
//! the picture alone: made from the frame's own interpolated poses and never
//! read by the simulation, so collision, aim and replays do not see them.
//!
//! Which renderer draws what: the body being played is drawn only in a chase
//! view (in first person the GPU still casts its shadow), and only by the
//! hardware renderer, which the session tells apart with
//! [`GameSession`](crate::session::GameSession)'s software cut. The ship parked
//! while the pilot is on foot is something play depends on (it is the way back),
//! so every renderer draws it.
//!
//! The blueprints are authored in the body's own frame (forward along -Z, up
//! along +Y), so the body's orientation turns them as they are. They are drawn
//! from the level's model table, beside the enemies, so a frame has one table.

use mscent_math::{Quat, Vec3};
use mscent_render::LevelRenderer;
use mscent_render::model::{ModelId, ModelInstance};
use mscent_world::model::{Blueprint, Model};

const SHIP: &str = include_str!("../../../assets/ship/player.msb");
const PILOT: &str = include_str!("../../../assets/ship/pilot.msb");

/// A design read once and the id it has in the current renderer's table.
struct Design {
    blueprint: Blueprint,
    model: Model,
    id: ModelId,
}

impl Design {
    fn build(text: &str, renderer: &mut LevelRenderer) -> Option<Self> {
        let blueprint = mscent_world::model::read(text.as_bytes()).ok()?;
        let model = mscent_world::model::build(&blueprint).ok()?;
        let id = renderer.add_model(&blueprint, &model);
        Some(Self {
            blueprint,
            model,
            id,
        })
    }
}

pub struct Crew {
    ship: Design,
    pilot: Design,
}

impl Crew {
    /// The ship and the pilot, added to `renderer`'s models with skins from the
    /// level's materials. `None` for a blueprint that does not read, which the
    /// tests hold them to.
    pub fn build(renderer: &mut LevelRenderer) -> Option<Self> {
        Some(Self {
            ship: Design::build(SHIP, renderer)?,
            pilot: Design::build(PILOT, renderer)?,
        })
    }

    /// Adds both to `renderer`, which replaces the one they were added to.
    pub fn add_to(&mut self, renderer: &mut LevelRenderer) {
        for d in [&mut self.ship, &mut self.pilot] {
            d.id = renderer.add_model(&d.blueprint, &d.model);
        }
    }

    /// The ship at `pos` turned as `orientation`, `scale` times its stock size.
    /// `drawn` false only casts a shadow (first person, where it would fill the
    /// view).
    pub fn ship(&self, pos: Vec3, orientation: Quat, scale: f32, drawn: bool) -> ModelInstance {
        ModelInstance {
            cast_only: !drawn,
            scale,
            ..ModelInstance::new(self.ship.id, pos, orientation)
        }
    }

    /// The pilot standing with the feet sphere's centre at `feet`, facing as
    /// `heading` (level and upright).
    pub fn pilot(&self, feet: Vec3, heading: Quat, drawn: bool) -> ModelInstance {
        ModelInstance {
            cast_only: !drawn,
            ..ModelInstance::new(self.pilot.id, feet, heading)
        }
    }
}

#[cfg(test)]
mod tests {
    use mscent_world::model::{build, read};

    #[test]
    fn the_ship_and_the_pilot_read_and_build() {
        for text in [super::SHIP, super::PILOT] {
            let blueprint = read(text.as_bytes()).expect("reads");
            let model = build(&blueprint).expect("builds");
            let tris: usize = model.parts.iter().map(|p| p.lods[0].triangles.len()).sum();
            assert!(
                tris > 100 && tris < 6000,
                "{} has {tris} triangles",
                blueprint.name
            );
        }
    }
}
