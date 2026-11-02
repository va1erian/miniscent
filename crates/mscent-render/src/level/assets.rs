//! What a level is, whichever renderer draws it: the scene, its light and
//! materials. Nothing here belongs to a frame, and nothing here rasterizes.

use std::sync::Arc;

use mscent_world::lightmap::{CellSites, Lightmap, PointLight};

use super::setup::{mesh_lighting, spec_tables};
use super::{Actor, BuildConfig, FAR, NEAR, View, VoxelChange, compiled, movers, overlay};
use crate::camera::Camera;
use crate::lighting_shader::UvToLuxel;
use crate::lightmap_mips::LightMips;
use crate::material::MaterialTable;
use crate::mesh::MeshLighting;
use crate::scene::Scene;
use crate::shade::Fog;
use crate::specular::SpecTable;

/// A level compiled and lit, ready for a renderer to take.
pub struct LevelAssets {
    pub(super) scene: Scene,
    pub(super) lightmap: Arc<Lightmap>,
    /// Pre-filtered lightmap levels, built with the assets.
    pub(super) light_mips: LightMips,
    pub(super) uv_maps: Vec<Option<UvToLuxel>>,
    pub(super) spec: Vec<SpecTable>,
    pub(super) lighting: MeshLighting,
    /// The level's lamps, for voxel surfaces made after the build.
    voxel_lights: Vec<PointLight>,
    /// The cells the lightmap holds records for.
    voxel_sites: CellSites,
    actors: Vec<Actor>,
    pub(super) meshes: overlay::ActorMeshes,
    pub(super) mover_faces: Vec<movers::MoverFaces>,
    cfg: BuildConfig,
}

impl LevelAssets {
    /// Builds the assets from what [`compiled::Compiled`] prepared, with
    /// materials [`super::check_materials`] accepted.
    pub(super) fn assemble(
        parts: compiled::Parts,
        lightmap: Arc<Lightmap>,
        materials: MaterialTable,
    ) -> Self {
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
            voxel_lights: lights,
            voxel_sites: sites,
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

    /// Changes voxel surfaces under the same brushes, between two frames.
    pub fn apply_voxels(&mut self, change: VoxelChange) {
        let (lights, ambient) = (&self.voxel_lights, self.cfg.ambient);
        let leaves = self.scene.bsp().leaves().len();
        let (sites, lightmap) = (&self.voxel_sites, &self.lightmap);
        self.scene.update_voxels(|voxels| match change {
            VoxelChange::Chunks(list) => {
                voxels.apply(list, |mesh| compiled::lamp_light(mesh, lights, ambient));
            }
            VoxelChange::All(world) => {
                *voxels = voxels.remade(*world, leaves, |mesh| {
                    compiled::lamp_light(mesh, lights, ambient)
                });
            }
            VoxelChange::Restore(world) => {
                *voxels = compiled::lit_voxels(*world, leaves, sites, lightmap);
            }
        });
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

    /// The camera a frame uses for this view and target size.
    pub fn camera(&self, view: &View, width: u32, height: u32) -> Option<Camera> {
        view.camera(self.cfg.fov_y, NEAR, FAR, width, height)
    }
}
