//! Choosing a frame's shadows: whether anything can cast, who holds the maps, and
//! the lists of what casts, made once for the frame.

use mscent_render::scene::{SceneStats, eye_of};
use mscent_render::{Camera, DynLight, FrameInput, LevelAssets, Quality};

use crate::GpuRenderer;
use crate::actors::ActorPass;
use crate::effects::GpuEffects;
use crate::faces::FacePass;
use crate::models::ModelPass;
use crate::shadows::{Casters, Picked};

/// What casts shadows in a frame, borrowed from the passes that listed it.
pub(crate) fn casters_of<'a>(
    actors: &'a ActorPass,
    models: &'a ModelPass,
    movers: &'a FacePass,
) -> Casters<'a> {
    Casters {
        actors: actors.casters(),
        actor_bound: actors.bound(),
        models: models.casters(),
        movers: movers.casters(),
    }
}

impl GpuRenderer {
    /// The maps this frame holds, with what casts into them listed. Nothing is
    /// listed, and nothing costs, without shadows switched on, a device that holds
    /// the maps, and a light or lamp to cast. `None` as for [`Self::draw`], when
    /// the lists could not be made.
    pub(super) fn pick_shadows(
        &mut self,
        assets: &LevelAssets,
        cam: &Camera,
        frame: &FrameInput,
        (quality, effects): (Quality, GpuEffects),
    ) -> Option<Picked> {
        let lights = frame.effects.lights;
        let lamps = assets.lamps();
        let casting = effects.shadows
            && self.shadows.ready()
            && (!lamps.is_empty() || lights.iter().any(DynLight::is_lit));
        if !casting {
            return Some(Picked::NONE);
        }
        let seen = (frame.effects.models, lights);
        let listed = self.gpu.checked(|| {
            let mut unused = SceneStats::default();
            (self.casters).prepare(&self.gpu, assets, (cam, quality), seen, &mut unused)
        });
        self.reported(listed)?;
        let casters = casters_of(&self.actors, &self.casters, &self.faces);
        let eye = eye_of(cam);
        Some(self.shadows.pick(&self.gpu, (lights, lamps), eye, &casters))
    }
}
