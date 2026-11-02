//! Filling a frame's faces and listing its actors, with the dynamic lights
//! of the frame added where they reach.

use mscent_math::Vec3;
use mscent_world::Face;
use mscent_world::materials::{LIGHTPANEL, is_self_lit};

use super::overlay::LitActors;
use super::shader::FaceShader;
use super::{Actor, EMISSIVE, FrameState, LevelRenderer, Quality, SELF_LIT, UNLIT};
use crate::camera::Camera;
use crate::dynlight::{DynLight, MAX_DYN_LIGHTS};
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::{FaceLight, LightingShader};
use crate::raster::{DrawStats, PolyList};
use crate::shade::{FlatShader, Fog};

/// Adds the lights of the frame that reach `face` to its shader.
fn add_lights<const SPEC: bool>(
    shader: &mut LightingShader<'_, SPEC>,
    face: usize,
    lights: &[DynLight],
) {
    for l in lights.iter().filter(|l| l.is_lit() && l.reaches_face(face)) {
        shader.add_point(l.pos(), l.radius(), l.color());
    }
}

impl LevelRenderer {
    /// Replaces `frame`'s actor list. Without a lit dynamic light this is the
    /// level's own lighting, untouched.
    pub(super) fn list_actors(
        &self,
        cam: &Camera,
        actors: &[Actor],
        lights: &[DynLight],
        frame: &mut FrameState,
    ) {
        if !lights.iter().any(DynLight::is_lit) {
            self.meshes
                .list(cam, actors, &self.lighting, &mut frame.actors);
            return;
        }
        let lighting = frame.lit.get_or_insert_with(|| {
            let mut copy = self.lighting.clone();
            copy.points.reserve(MAX_DYN_LIGHTS);
            copy
        });
        let lit = LitActors {
            scene: &self.scene,
            lights,
            lighting,
            base: self.lighting.points.len(),
        };
        self.meshes.list_lit(cam, actors, lit, &mut frame.actors);
    }

    pub(super) fn fill_level(
        &self,
        fb: &mut Framebuffer,
        list: &PolyList,
        quality: Quality,
        eye: Vec3,
        lights: &[DynLight],
    ) -> DrawStats {
        self.scene.fill_frame(fb, list, None, &mut |i, face| {
            self.face_shader(i, face, eye, quality, lights, self.fog())
        })
    }

    /// The lit shader of a face, seen from `eye` in the space the face was
    /// authored in.
    pub(super) fn face_shader(
        &self,
        i: usize,
        face: &Face,
        eye: Vec3,
        quality: Quality,
        lights: &[DynLight],
        fog: Option<Fog>,
    ) -> FaceShader<'_> {
        let flat = |light| FaceShader::Flat(FlatShader::new(light, fog));
        if face.material == LIGHTPANEL {
            return flat(EMISSIVE);
        }
        // No luxels to look up: a sky shows its texture as it is.
        if is_self_lit(face.material) {
            return flat(SELF_LIT);
        }
        let map = self.uv_maps.get(i).copied().flatten();
        let Some(map) = map else {
            return flat(UNLIT);
        };
        let fl = FaceLight {
            lightmap: &self.lightmap,
            mips: &self.light_mips,
            face: i,
            map,
            texture: self.scene.texture_of(face.material),
            fog,
            bump: quality != Quality::Low,
        };
        let table = self
            .spec
            .get(usize::from(face.material))
            .or(self.spec.last());
        let spec = table
            .filter(|t| quality == Quality::High && t.is_visible())
            .and_then(|t| LightingShader::with_specular(fl, eye, t));
        if let Some(mut s) = spec {
            add_lights(&mut s, i, lights);
            return FaceShader::Spec(s);
        }
        match LightingShader::new(fl) {
            Some(mut s) => {
                add_lights(&mut s, i, lights);
                FaceShader::Lit(s)
            }
            None => flat(UNLIT),
        }
    }
}
