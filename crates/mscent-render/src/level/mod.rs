//! High-level level rendering: from a [`Level`] (brushes and entities) to
//! lit, textured frames.
//!
//! [`LevelRenderer::from_level`] runs the whole pipeline once: CSG to BSP,
//! face extraction, portals, lightmap bake (point lights, shadows, ambient
//! and bounce) and the per-face mapping from texture to lightmap
//! coordinates. [`LevelRenderer::render`] then draws a frame through the
//! portal-culled BSP walk with the baked light, bump mapping and specular
//! chosen by [`Quality`], distance fog, a sky gradient behind everything,
//! the level's enemy and goal meshes and an optional HUD.

mod cache;
mod compiled;
mod overlay;
mod shader;
pub mod view;

use std::sync::Arc;

use mscent_math::Vec3;
use mscent_world::lightmap::{Lightmap, LightmapError, PointLight};
use mscent_world::materials::{self, LIGHTPANEL};
use mscent_world::{CsgError, Entity, Level};

use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::{FaceLight, LightingShader, UvToLuxel};
use crate::lightmap_mips::LightMips;
use crate::mesh::{MeshLighting, PointLight as MeshLight};
use crate::scene::{Scene, SceneStats, eye_of};
use crate::shade::{FlatShader, Fog};
use crate::specular::SpecTable;
use crate::texture::{MaterialId, MaterialTable};
use shader::FaceShader;

pub use cache::{BakeCache, BakeReport, BakedLight, Source};
pub use compiled::Compiled;
pub use overlay::{Actor, ActorKind, Overlay};
pub use view::View;

/// Light scale of emissive surfaces (256 is the plain texture colour).
const EMISSIVE: u32 = 330;
/// Light scale of a face that has no lightmap data.
const UNLIT: u32 = 200;
const NEAR: f32 = 0.1;
const FAR: f32 = 400.0;

/// How much of the lighting model a frame uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// Lightmap shading only.
    Low,
    /// Lightmap and bump mapping.
    Medium,
    /// Lightmap, bump mapping and specular highlights.
    High,
}

/// Bake and look settings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BuildConfig {
    /// World units per luxel.
    pub luxel_size: f32,
    pub ambient: Vec3,
    /// Multiplier on every point light's colour (the baked light then
    /// saturates the texture, which is what makes bright pools).
    pub light_gain: f32,
    /// Radius of the lamp disc for soft shadows, in world units (0: hard).
    pub light_radius: f32,
    /// Shadow rays per luxel and light for soft shadows.
    pub shadow_samples: u32,
    /// Colours of the sky gradient; the bottom one is also the fog colour.
    pub sky_top: u32,
    pub sky_bottom: u32,
    /// View depths where fog starts and where it is total.
    pub fog_start: f32,
    pub fog_end: f32,
    /// Vertical field of view in radians.
    pub fov_y: f32,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            luxel_size: 0.125,
            ambient: Vec3::new(0.025, 0.024, 0.036),
            light_gain: 4.6,
            light_radius: 0.3,
            shadow_samples: 64,
            sky_top: 0xff10_2048,
            sky_bottom: 0xff3a_4a78,
            fog_start: 4.0,
            fog_end: 26.0,
            fov_y: 1.3,
        }
    }
}

/// Why a level could not be prepared for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildError {
    Csg(CsgError),
    Bake(LightmapError),
    /// The material table does not hold the standard textures in id order;
    /// the value is the first id that is wrong.
    Materials(u16),
}

/// A level compiled, lit and ready to draw.
pub struct LevelRenderer {
    scene: Scene,
    lightmap: Arc<Lightmap>,
    /// Pre-filtered lightmap levels, built with the renderer.
    light_mips: LightMips,
    uv_maps: Vec<Option<UvToLuxel>>,
    spec: Vec<SpecTable>,
    lighting: MeshLighting,
    actors: Vec<Actor>,
    meshes: overlay::ActorMeshes,
    cfg: BuildConfig,
}

fn check_materials(table: &MaterialTable) -> Result<(), BuildError> {
    for (id, name) in (0u16..).zip(materials::NAMES) {
        if table.find(name) != Some(MaterialId(id)) {
            return Err(BuildError::Materials(id));
        }
    }
    Ok(())
}

fn lights_of(level: &Level, gain: f32) -> Vec<PointLight> {
    level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Light {
                pos,
                color,
                radius,
                intensity,
            } => Some(PointLight {
                position: pos,
                color: color * (intensity * gain),
                radius,
            }),
            _ => None,
        })
        .collect()
}

fn actors_of(level: &Level) -> Vec<Actor> {
    let actor = |kind, pos| Actor {
        kind,
        pos,
        yaw: 0.0,
    };
    level
        .entities
        .iter()
        .filter_map(|e| match *e {
            Entity::Enemy { pos, .. } => Some(actor(ActorKind::Enemy, pos)),
            Entity::Goal { pos, .. } => Some(actor(ActorKind::Goal, pos)),
            _ => None,
        })
        .collect()
}

/// Entities cannot sample the lightmap (it is per face), so they are lit by
/// the same point lights without shadows plus a little ambient.
fn mesh_lighting(lights: &[PointLight], ambient: Vec3) -> MeshLighting {
    MeshLighting {
        ambient: 0.4 + (ambient.x + ambient.y + ambient.z) / 3.0,
        sun_dir: Vec3::new(0.3, -0.4, 0.85)
            .normalized()
            .unwrap_or(Vec3::ZERO),
        sun: 0.7,
        points: lights
            .iter()
            .map(|l| MeshLight {
                pos: l.position,
                radius: l.radius,
                intensity: (l.color.x + l.color.y + l.color.z) / 3.0,
            })
            .collect(),
    }
}

/// One specular table per material id, plus a last one for unknown ids.
fn spec_tables(table: &MaterialTable) -> Vec<SpecTable> {
    let mut tables: Vec<SpecTable> = (0u16..)
        .take(table.len())
        .map(|i| SpecTable::new(table.get(MaterialId(i)).map_or(40, |m| m.gloss)))
        .collect();
    tables.push(SpecTable::new(40));
    tables
}

impl LevelRenderer {
    /// Compile and bake `level`, drawing with `materials` (see
    /// [`mscent_world::materials`] for the expected ids).
    pub fn from_level(
        level: &Level,
        materials: MaterialTable,
        cfg: &BuildConfig,
    ) -> Result<Self, BuildError> {
        Self::from_level_cached(level, materials, cfg, &mut BakeCache::new())
    }

    /// Like [`Self::from_level`], but the bake goes through `cache`: when the
    /// faces, lights and bake settings are those of the cache's last bake (or
    /// of its file), no shadow ray is traced and the same lightmap is shared.
    pub fn from_level_cached(
        level: &Level,
        materials: MaterialTable,
        cfg: &BuildConfig,
        cache: &mut BakeCache,
    ) -> Result<Self, BuildError> {
        check_materials(&materials)?;
        let compiled = Compiled::new(level, cfg)?;
        let lightmap = compiled.get_or_bake(cache)?;
        compiled.into_renderer(lightmap, materials)
    }

    /// Builds the renderer from what [`Compiled`] prepared, with materials
    /// [`check_materials`] accepted.
    fn assemble(parts: compiled::Parts, lightmap: Arc<Lightmap>, materials: MaterialTable) -> Self {
        let compiled::Parts {
            bsp,
            faces,
            portals,
            lights,
            actors,
            cfg,
        } = parts;
        let spec = spec_tables(&materials);
        // The game path pays for smooth texture sampling; the raw
        // rasterizer benchmark does not.
        let scene = Scene::new(bsp, faces, portals, materials.filtered());
        let uv_maps = (0..scene.faces().len())
            .map(|i| {
                let proj = lightmap.projections().get(i)?;
                let face = scene.faces().get(i)?;
                let verts = scene.polys().get(i)?;
                UvToLuxel::from_vertices(proj, verts, scene.texture_of(face.material))
            })
            .collect();
        Self {
            lighting: mesh_lighting(&lights, cfg.ambient),
            actors,
            meshes: overlay::ActorMeshes::new(),
            scene,
            light_mips: LightMips::build(&lightmap),
            lightmap,
            uv_maps,
            spec,
            cfg,
        }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn lightmap(&self) -> &Lightmap {
        &self.lightmap
    }

    pub fn config(&self) -> &BuildConfig {
        &self.cfg
    }

    /// The level's own enemies and goal, at their spawn points.
    pub fn actors(&self) -> &[Actor] {
        &self.actors
    }

    fn fog(&self) -> Option<Fog> {
        Fog::new(self.cfg.sky_bottom, self.cfg.fog_start, self.cfg.fog_end)
    }

    /// Draw one frame: sky, level, `actors`, then the HUD. `None` when the
    /// view or the framebuffer size cannot make a camera.
    pub fn render(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
    ) -> Option<SceneStats> {
        let cam = view.camera(self.cfg.fov_y, NEAR, FAR, fb.width(), fb.height())?;
        fb.clear_sky(self.cfg.sky_top, self.cfg.sky_bottom);
        let stats = self.draw_level(fb, &cam, quality);
        self.meshes
            .draw(fb, &cam, actors, &self.lighting, self.fog());
        if let Some(o) = hud {
            overlay::draw_overlay(fb, o);
        }
        Some(stats)
    }

    fn draw_level(&self, fb: &mut Framebuffer, cam: &Camera, quality: Quality) -> SceneStats {
        let eye = eye_of(cam);
        let fog = self.fog();
        let bump = quality != Quality::Low;
        let specular = quality == Quality::High;
        let flat = |light| FaceShader::Flat(FlatShader::new(light, fog));
        self.scene.render_with(fb, cam, None, &mut |i, face| {
            if face.material == LIGHTPANEL {
                return flat(EMISSIVE);
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
                bump,
            };
            let table = self
                .spec
                .get(usize::from(face.material))
                .or(self.spec.last());
            let spec = table
                .filter(|t| specular && t.is_visible())
                .and_then(|t| LightingShader::with_specular(fl, eye, t));
            if let Some(s) = spec {
                return FaceShader::Spec(s);
            }
            LightingShader::new(fl).map_or_else(|| flat(UNLIT), FaceShader::Lit)
        })
    }
}
