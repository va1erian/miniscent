//! What a level is, whichever renderer draws it: the scene, its light and
//! materials. Nothing here belongs to a frame, and nothing here rasterizes.

use std::sync::Arc;

use mscent_math::Vec3;

use mscent_world::lightmap::{CellSites, Lightmap, PointLight};

use super::setup::{mesh_lighting, spec_tables};
use super::vista;
use super::{Actor, BuildConfig, FAR, NEAR, View, VoxelChange, compiled, movers, overlay};
use crate::camera::Camera;
use crate::dynlight::{DynLight, MAX_DYN_LIGHTS};
use crate::lighting_shader::UvToLuxel;
use crate::lightmap_mips::LightMips;
use crate::material::MaterialTable;
use crate::mesh::MeshLighting;
use crate::model::{ModelInstance, ModelList, ModelTable};
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
    /// Every lamp, and those of the play space, for voxel surfaces made after
    /// the build: a layer's room is lit by its own lamps and the play space not.
    voxel_lights: Vec<PointLight>,
    play_lights: Vec<PointLight>,
    /// The cells the lightmap holds records for.
    voxel_sites: CellSites,
    actors: Vec<Actor>,
    pub(super) meshes: overlay::ActorMeshes,
    pub(super) mover_faces: Vec<movers::MoverFaces>,
    pub(super) vistas: vista::Vistas,
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
            holes,
            vistas,
            mover_ranges,
            portals,
            lights: all_lights,
            play_lights: lights,
            actors,
            cfg,
            voxels,
            sites,
        } = parts;
        let spec = spec_tables(&materials);
        // The game path pays for smooth texture sampling; the raw
        // rasterizer benchmark does not.
        let voxels = compiled::lit_voxels(voxels, bsp.leaves().len(), &sites, &lightmap);
        let scene = Scene::with_voxels(bsp, faces, portals, materials.filtered(), voxels)
            .with_holes(&holes);
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
            vistas,
            lighting: mesh_lighting(&lights, cfg.ambient),
            voxel_lights: all_lights,
            play_lights: lights,
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
        let ambient = self.cfg.ambient;
        let leaves = self.scene.bsp().leaves().len();
        let (sites, lightmap) = (&self.voxel_sites, &self.lightmap);
        let (all, play, vistas) = (&self.voxel_lights, &self.play_lights, &self.vistas);
        self.scene.update_voxels(|voxels, bsp| {
            let lamps = |pos| vistas.lamps_at(bsp, pos, all, play);
            match change {
                VoxelChange::Chunks(list) => {
                    voxels.apply(list, |mesh| compiled::lamp_light(mesh, lamps, ambient));
                }
                VoxelChange::All(world) => {
                    *voxels = voxels.remade(*world, leaves, |mesh| {
                        compiled::lamp_light(mesh, lamps, ambient)
                    });
                }
                VoxelChange::Restore(world) => {
                    *voxels = compiled::lit_voxels(*world, leaves, sites, lightmap);
                }
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

    /// The pre-filtered light of every face, finest level first.
    pub fn light_mips(&self) -> &LightMips {
        &self.light_mips
    }

    /// The fit from a face's texels to its luxels; `None` draws it unlit.
    pub fn uv_map(&self, face: usize) -> Option<UvToLuxel> {
        self.uv_maps.get(face).copied().flatten()
    }

    /// The power table of a material.
    pub fn spec_table(&self, material: u16) -> Option<&SpecTable> {
        self.spec.get(usize::from(material)).or(self.spec.last())
    }

    /// Each mover's faces, by its order in the level.
    pub fn movers(&self) -> &[movers::MoverFaces] {
        &self.mover_faces
    }

    /// Each triangle of `actors` that faces `eye`: its corners in world space
    /// with their light factors (see [`MeshLighting::at`]), and its flat colour.
    /// The first [`MAX_DYN_LIGHTS`] of `lights` light the actors they reach, as
    /// in a software frame.
    pub fn actor_triangles(
        &self,
        actors: &[Actor],
        eye: Vec3,
        lights: &[DynLight],
        visit: impl FnMut([(Vec3, f32); 3], u32),
    ) {
        let lights = &lights[..lights.len().min(MAX_DYN_LIGHTS)];
        if !lights.iter().any(DynLight::is_lit) {
            self.meshes
                .each_triangle(actors, &self.lighting, eye, visit);
            return;
        }
        let mut lighting = self.lighting.clone();
        let lit = overlay::LitActors {
            scene: &self.scene,
            lights,
            base: lighting.points.len(),
            lighting: &mut lighting,
        };
        self.meshes.each_triangle_lit(actors, lit, eye, visit);
    }

    /// Lists the parts of `instances` that `cam` sees, lit by the level and by the
    /// first [`MAX_DYN_LIGHTS`] of `lights` where they reach, as a software frame
    /// lists them; `None` empties it.
    pub fn list_models(
        &self,
        list: &mut ModelList,
        models: Option<(&ModelTable, &[ModelInstance])>,
        cam: &Camera,
        lights: &[DynLight],
    ) {
        let Some((table, instances)) = models else {
            list.clear();
            return;
        };
        let lights = &lights[..lights.len().min(MAX_DYN_LIGHTS)];
        let bsp = self.scene.bsp();
        list.prepare(table, instances, cam, &self.lighting, lights, |p| {
            bsp.leaf_at_f32(p)
        });
    }

    /// Lists every part of `instances` for a shadow map, whatever `cam` sees and
    /// including the instances that are only cast, with no dynamic light: what a
    /// shadow needs of a model is its shape. `list` is from
    /// [`ModelList::casters_unprojected`].
    pub fn list_model_casters(
        &self,
        list: &mut ModelList,
        models: Option<(&ModelTable, &[ModelInstance])>,
        cam: &Camera,
    ) {
        self.list_models(list, models, cam, &[]);
    }

    /// The camera a frame uses for this view and target size.
    pub fn camera(&self, view: &View, width: u32, height: u32) -> Option<Camera> {
        view.camera(self.cfg.fov_y, NEAR, FAR, width, height)
    }
}
