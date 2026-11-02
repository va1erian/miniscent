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
mod effects;
mod fill;
mod movers;
mod overlay;
mod setup;
mod shader;
pub mod view;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use mscent_math::Vec3;
use mscent_world::lightmap::{Lightmap, LightmapError};
use mscent_world::materials;
use mscent_world::{CsgError, Level};

use crate::camera::Camera;
use crate::dynlight::MAX_DYN_LIGHTS;
use crate::framebuffer::Framebuffer;
use crate::lighting_shader::UvToLuxel;
use crate::lightmap_mips::LightMips;
use crate::mesh::MeshLighting;
use crate::particles::{ParticleDraws, SpriteBank, draw_particles};
use crate::raster::PolyList;
use crate::scene::{Scene, SceneStats, eye_of};
use crate::shade::Fog;
use crate::specular::SpecTable;
use crate::texture::{MaterialId, MaterialTable};
use crate::tiles::TileRenderer;
use crate::voxel::fill::{FillContext, fill_voxels};
use crate::voxel::list::VoxelList;

pub use cache::{BakeCache, BakeReport, BakedLight, Source};
pub use compiled::Compiled;
pub use effects::Effects;
pub use movers::MoverPose;
pub use overlay::{Actor, ActorKind, Overlay};
use setup::{mesh_lighting, spec_tables};
pub use view::View;

/// Light scale of emissive surfaces (256 is the plain texture colour).
const EMISSIVE: u32 = 330;
/// Light scale of a face that has no lightmap data.
const UNLIT: u32 = 200;
const NEAR: f32 = 0.1;
const FAR: f32 = 400.0;
/// Rows per band of a frame.
const TILE_ROWS: u32 = 24;
/// Fewest workers a renderer starts with, whatever the CPU reports.
const MIN_WORKERS: usize = 4;

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
            ambient: Vec3::new(0.006, 0.006, 0.009),
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
    mover_faces: Vec<movers::MoverFaces>,
    cfg: BuildConfig,
    /// Band height and worker count of [`Self::render`].
    tiling: (u32, usize),
    frame: Mutex<Option<FrameState>>,
}

/// What [`LevelRenderer::render`] keeps between frames, so a frame does not
/// remake it: the bands, the worker threads and the polygon lists' storage.
struct FrameState {
    tiles: TileRenderer,
    faces: PolyList,
    actors: PolyList,
    /// The level's mesh lighting plus this frame's dynamic lights, made on
    /// the first frame that has one and reused after it.
    lit: Option<MeshLighting>,
    movers: movers::MoverList,
    voxels: VoxelList,
}

fn check_materials(table: &MaterialTable) -> Result<(), BuildError> {
    for (id, name) in (0u16..).zip(materials::NAMES) {
        if table.find(name) != Some(MaterialId(id)) {
            return Err(BuildError::Materials(id));
        }
    }
    Ok(())
}

/// Workers of a new renderer: every hardware thread, and at least
/// [`MIN_WORKERS`].
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map_or(MIN_WORKERS, usize::from)
        .max(MIN_WORKERS)
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
            mover_ranges,
            portals,
            lights,
            actors,
            cfg,
            voxels,
            sites,
        } = parts;
        let spec = spec_tables(&materials);
        // The game path pays for smooth texture sampling; the raw
        // rasterizer benchmark does not.
        let voxels = compiled::lit_voxels(voxels, bsp.leaves().len(), &sites, &lightmap);
        let scene = Scene::with_voxels(bsp, faces, portals, materials.filtered(), voxels);
        let uv_maps = (0..scene.faces().len())
            .map(|i| {
                let proj = lightmap.projections().get(i)?;
                let face = scene.faces().get(i)?;
                let verts = scene.polys().get(i)?;
                UvToLuxel::from_vertices(proj, verts, scene.texture_of(face.material))
            })
            .collect();
        let mover_faces = mover_ranges
            .into_iter()
            .map(|faces| {
                let polys = scene.polys().get(faces.clone()).unwrap_or(&[]);
                let (centre, radius) = movers::bounding_sphere(polys);
                movers::MoverFaces {
                    faces,
                    centre,
                    radius,
                }
            })
            .collect();
        Self {
            mover_faces,
            lighting: mesh_lighting(&lights, cfg.ambient),
            actors,
            meshes: overlay::ActorMeshes::new(),
            scene,
            light_mips: LightMips::build(&lightmap),
            lightmap,
            uv_maps,
            spec,
            cfg,
            tiling: (TILE_ROWS, default_workers()),
            frame: Mutex::new(None),
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

    /// The level's fog, for effects drawn in the same frame.
    pub fn fog(&self) -> Option<Fog> {
        Fog::new(self.cfg.sky_bottom, self.cfg.fog_start, self.cfg.fog_end)
    }

    /// Band height and worker count of later frames (workers are clamped to
    /// `1..=MAX_WORKERS`, rows to at least one).
    pub fn set_tiling(&mut self, rows: u32, workers: usize) {
        self.tiling = (rows.max(1), workers);
        *self.frame.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The camera [`Self::render`] uses for this view and target size.
    pub fn camera(&self, view: &View, width: u32, height: u32) -> Option<Camera> {
        view.camera(self.cfg.fov_y, NEAR, FAR, width, height)
    }

    /// Draw one frame: visibility and projection once, then sky, level and
    /// `actors` in parallel bands, then the HUD. `None` when the view or the
    /// framebuffer size cannot make a camera.
    pub fn render(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
    ) -> Option<SceneStats> {
        self.render_with_effects(fb, view, quality, actors, hud, &Effects::default())
    }

    /// The same, with movers at `poses`, by their order in the level. A mover
    /// with no entry is drawn as authored.
    pub fn render_posed(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
        poses: &[MoverPose],
    ) -> Option<SceneStats> {
        let effects = Effects {
            poses,
            ..Effects::default()
        };
        self.render_with_effects(fb, view, quality, actors, hud, &effects)
    }

    /// [`Self::render`] with particles over the finished scene. They are part
    /// of the same pass over the bands, so they cost no second dispatch.
    pub fn render_with_particles(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
        particles: Option<(&ParticleDraws, &SpriteBank)>,
    ) -> Option<SceneStats> {
        let effects = Effects {
            particles,
            ..Effects::default()
        };
        self.render_with_effects(fb, view, quality, actors, hud, &effects)
    }

    /// [`Self::render`] with `effects` in the same pass: movers where they are
    /// now, particles over the finished scene, and dynamic lights on the walls
    /// and actors they reach. Of the lights only the first [`MAX_DYN_LIGHTS`]
    /// count.
    pub fn render_with_effects(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
        effects: &Effects,
    ) -> Option<SceneStats> {
        let lights = &effects.lights[..effects.lights.len().min(MAX_DYN_LIGHTS)];
        let cam = self.camera(view, fb.width(), fb.height())?;
        let mut slot = self.frame.lock().unwrap_or_else(PoisonError::into_inner);
        let frame = self.frame_for(&mut slot, fb.width(), fb.height())?;
        let mut stats = self.scene.list_frame_voxels(
            &cam,
            None,
            &mut frame.faces,
            &mut frame.voxels.candidates,
        );
        frame
            .voxels
            .prepare(self.scene.voxels(), &cam, eye_of(&cam), |leaf| {
                let lit = lights.iter().enumerate();
                lit.fold(0u8, |m, (i, l)| {
                    if l.is_lit() && l.reaches_leaf(leaf) {
                        m | (1 << i)
                    } else {
                        m
                    }
                })
            });
        stats.voxel_triangles = frame.voxels.triangles;
        self.list_movers(&cam, effects.poses, &mut frame.movers);
        self.list_actors(&cam, actors, lights, frame);
        let (faces, meshes, movers) = (&frame.faces, &frame.actors, &frame.movers);
        let voxels = &frame.voxels;
        let (pixels, spans) = (AtomicU32::new(0), AtomicU32::new(0));
        let eye = eye_of(&cam);
        let texture_of = |id: u16| self.scene.texture_of(id);
        let voxel_fill = FillContext {
            scene: self.scene.voxels(),
            cam: &cam,
            eye,
            fog: self.fog(),
            bump: quality != Quality::Low,
            specular: quality == Quality::High,
            lights,
            spec: &self.spec,
            texture_of: &texture_of,
        };
        frame.tiles.pass(&|band: &mut Framebuffer| {
            band.clear_sky(self.cfg.sky_top, self.cfg.sky_bottom);
            // First, so the z-buffer saves shading whatever a door covers.
            let moved = self.fill_movers(band, movers, quality);
            // Next, for the same reason: a voxel pixel costs less than a wall's.
            let vox = fill_voxels(band, voxels, &voxel_fill);
            let st = self.fill_level(band, faces, quality, eye, lights);
            self.meshes.fill(band, meshes, self.fog());
            if let Some((draws, bank)) = effects.particles {
                draw_particles(band, draws, bank);
            }
            pixels.fetch_add(moved.pixels + vox.pixels + st.pixels, Ordering::Relaxed);
            spans.fetch_add(moved.spans + vox.spans + st.spans, Ordering::Relaxed);
        });
        frame.tiles.composite(fb).ok()?;
        drop(slot);
        if let Some(o) = hud {
            overlay::draw_overlay(fb, o);
        }
        stats.pixels = pixels.into_inner();
        stats.spans = spans.into_inner();
        Some(stats)
    }

    /// The per-frame state for a target of this size, made on first use so a
    /// renderer that never draws has no threads.
    fn frame_for<'a>(
        &self,
        slot: &'a mut Option<FrameState>,
        width: u32,
        height: u32,
    ) -> Option<&'a mut FrameState> {
        if slot.is_none() {
            *slot = Some(FrameState {
                tiles: TileRenderer::new(width, height, self.tiling.0, self.tiling.1)?,
                faces: PolyList::default(),
                actors: PolyList::default(),
                lit: None,
                movers: movers::MoverList::default(),
                voxels: VoxelList::default(),
            });
        }
        let frame = slot.as_mut()?;
        if frame.tiles.size() != (width, height) {
            frame.tiles.resize(width, height)?;
        }
        Some(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_renderer_gets_at_least_the_minimum_workers() {
        assert!(default_workers() >= MIN_WORKERS);
    }
}
