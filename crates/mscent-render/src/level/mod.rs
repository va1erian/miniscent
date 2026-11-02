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

mod assets;
mod cache;
mod compiled;
mod effects;
mod fill;
mod frame;
mod movers;
mod overlay;
mod setup;
mod shader;
pub mod view;

use std::sync::{Mutex, PoisonError};

use mscent_math::Vec3;
use mscent_world::lightmap::{Lightmap, LightmapError};
use mscent_world::materials;
use mscent_world::voxel::{RemeshedChunk, VoxelWorld};
use mscent_world::{CsgError, Level};

use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::material::{MaterialId, MaterialTable};
use crate::mesh::MeshLighting;
use crate::particles::{ParticleDraws, SpriteBank};
use crate::raster::PolyList;
use crate::scene::{Scene, SceneStats};
use crate::shade::Fog;
use crate::tiles::TileRenderer;
use crate::voxel::list::VoxelList;

pub use assets::LevelAssets;
pub use cache::{BakeCache, BakeReport, BakedLight, Source};
pub use compiled::Compiled;
pub use effects::Effects;
pub use frame::{FrameInput, FrameRenderer};
pub use movers::MoverPose;
pub use overlay::{Actor, ActorKind, Overlay};
pub use view::View;

/// Light scale of emissive surfaces (256 is the plain texture colour).
pub const EMISSIVE: u32 = 330;
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
    /// The material table does not hold the stock materials at their ids; the
    /// value is the first id that is wrong.
    Materials(u16),
}

/// A change to a level's voxel surfaces that needs no compile.
pub enum VoxelChange {
    /// The same boxes, with these chunks new.
    Chunks(Vec<RemeshedChunk>),
    /// Everything new: an add volume came, went or grew.
    All(Box<VoxelWorld>),
    /// The surfaces the lightmap was baked for, back again after damage.
    Restore(Box<VoxelWorld>),
}

/// The software renderer of a level.
pub struct LevelRenderer {
    assets: LevelAssets,
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

    /// The renderer of `assets`, which starts no threads until its first frame.
    pub fn from_assets(assets: LevelAssets) -> Self {
        Self {
            assets,
            tiling: (TILE_ROWS, default_workers()),
            frame: Mutex::new(None),
        }
    }

    /// The level again, for another renderer to take.
    pub fn into_assets(self) -> LevelAssets {
        self.assets
    }

    pub fn scene(&self) -> &Scene {
        self.assets.scene()
    }

    /// Changes voxel surfaces under the same brushes, between two frames.
    pub fn apply_voxels(&mut self, change: VoxelChange) {
        self.assets.apply_voxels(change);
    }

    pub fn lightmap(&self) -> &Lightmap {
        self.assets.lightmap()
    }

    pub fn config(&self) -> &BuildConfig {
        self.assets.config()
    }

    /// The level's own enemies and goal, at their spawn points.
    pub fn actors(&self) -> &[Actor] {
        self.assets.actors()
    }

    /// The level's fog, for effects drawn in the same frame.
    pub fn fog(&self) -> Option<Fog> {
        self.assets.fog()
    }

    /// Band height and worker count of later frames (workers are clamped to
    /// `1..=MAX_WORKERS`, rows to at least one).
    pub fn set_tiling(&mut self, rows: u32, workers: usize) {
        self.tiling = (rows.max(1), workers);
        *self.frame.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The camera [`Self::render`] uses for this view and target size.
    pub fn camera(&self, view: &View, width: u32, height: u32) -> Option<Camera> {
        self.assets.camera(view, width, height)
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
    /// and actors they reach.
    pub fn render_with_effects(
        &self,
        fb: &mut Framebuffer,
        view: &View,
        quality: Quality,
        actors: &[Actor],
        hud: Option<&Overlay>,
        effects: &Effects,
    ) -> Option<SceneStats> {
        let input = FrameInput {
            view,
            quality,
            actors,
            effects: *effects,
        };
        let stats = self.draw_frame(fb, &input)?;
        if let Some(o) = hud {
            overlay::draw_overlay(fb, o);
        }
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
