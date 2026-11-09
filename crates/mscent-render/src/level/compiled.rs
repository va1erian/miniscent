//! A level between its two costs: CSG, faces and portals are done, the light
//! is not. [`LevelRenderer::from_level_cached`] goes straight through; an
//! editor stops here so the picture can follow every edit: it asks for a stored
//! lightmap, falls back to a draft that takes milliseconds, and leaves the full
//! bake to a thread that can be cancelled and holds no lock on the cache.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mscent_math::Vec3;
use mscent_world::lightmap::store::input_key;
use mscent_world::lightmap::{
    BakeConfig, CellRecord, CellSites, LightFace, Lightmap, LightmapError, Occluders, PointLight,
    bake_in, shape_key,
};
use mscent_world::materials::{NO_DRAW, view_slot};
use mscent_world::voxel::{ChunkMesh, VoxelWorld};
use mscent_world::{Bsp, Face, Level, Portals, brush_faces, compile, extract_faces};

use super::kinds::{Design, designs_of, spawns_of};
use super::setup::{actors_of, lights_of, with_level_fog};
use super::vista::Vistas;
use super::{
    Actor, BakeCache, BakedLight, BuildConfig, BuildError, LevelAssets, LevelRenderer,
    check_materials,
};
use crate::material::MaterialTable;
use crate::voxel::{VertexLight, VoxelScene};

/// Draft luxel size; the baker coarsens it further if the atlas asks for that.
const DRAFT_LUXEL: f32 = 1.0;

/// Appends every mover's faces, lit at the pose they were authored in.
fn append_mover_faces(level: &Level, faces: &mut Vec<Face>) -> Vec<Range<usize>> {
    let mut ranges = Vec::with_capacity(level.mover_groups.len());
    for group in &level.mover_groups {
        let start = faces.len();
        for &index in &group.brushes {
            if let Some(brush) = level.brushes.get(index as usize) {
                faces.extend(brush_faces(brush));
            }
        }
        ranges.push(start..faces.len());
    }
    ranges
}

/// Moves the viewport faces out of `faces`, before any light is laid out for them.
fn take_viewport_faces(faces: &mut Vec<Face>) -> Vec<(Face, u8)> {
    let (holes, rest): (Vec<Face>, Vec<Face>) = core::mem::take(faces)
        .into_iter()
        .partition(|f| view_slot(f.material).is_some());
    *faces = rest;
    holes
        .into_iter()
        .filter_map(|f| view_slot(f.material).map(|slot| (f, slot)))
        .collect()
}

/// What the renderer keeps of a compiled level.
pub(super) struct Parts {
    pub bsp: Bsp,
    /// The world's faces, then each mover's.
    pub faces: Vec<Face>,
    /// Viewport faces with their slots, kept apart: no luxels, no texture.
    pub holes: Vec<(Face, u8)>,
    pub vistas: Vistas,
    /// For each mover, which of `faces` are its own.
    pub mover_ranges: Vec<Range<usize>>,
    pub portals: Portals,
    /// Every lamp, which the bake takes, since solid stops the rays of a far one.
    pub lights: Vec<PointLight>,
    /// The lamps of the play space: meshes and voxels light without shadows.
    pub play_lights: Vec<PointLight>,
    pub actors: Vec<Actor>,
    /// The enemy kinds, built, and where the level's enemies start.
    pub designs: Vec<Design>,
    pub spawns: Vec<(mscent_math::Vec3, u8)>,
    pub cfg: BuildConfig,
    pub voxels: VoxelWorld,
    /// The surface cells of `voxels`, which the lightmap holds a record for each of.
    pub sites: CellSites,
}

/// The light of a mesh's vertices: the record of the cell each stands in, or of
/// the cell a merged vertex was named for. A vertex with no record, which only a
/// lightmap of another level can cause, is dark rather than wrong.
fn vertex_light(
    sites: &CellSites,
    records: &[CellRecord],
    chunk: usize,
    mesh: &ChunkMesh,
) -> Vec<VertexLight> {
    mesh.vertices
        .iter()
        .map(|v| {
            let record = sites
                .lookup(chunk, v.cell)
                .and_then(|i| records.get(i))
                .copied()
                .unwrap_or_default();
            // No light arrived: the normal keeps the bump maps flat.
            let dir = Some(record.dir()).filter(|d| d.length_squared() > 0.0);
            VertexLight {
                color: record.color() * 256.0,
                dir: dir.unwrap_or(v.normal),
            }
        })
        .collect()
}

/// Lamps and ambient with no shadows: for a surface no bake has seen, until the
/// next one has. Nothing is traced, so it is fit for a stroke and for play.
/// `lamps` says which lamps light a point: the play space's, or every one in a
/// layer's room, whose own lamps are what its surfaces are baked with.
pub(super) fn lamp_light<'a>(
    mesh: &ChunkMesh,
    lamps: impl Fn(Vec3) -> &'a [PointLight],
    ambient: Vec3,
) -> Vec<VertexLight> {
    mesh.vertices
        .iter()
        .map(|v| {
            let mut color = ambient;
            let mut dir = Vec3::ZERO;
            for l in lamps(v.pos) {
                let to = l.position - v.pos;
                let d = to.length();
                if d <= 0.0 || d >= l.radius {
                    continue;
                }
                let beam = l.cone.map_or(1.0, |c| c.share(to * (-1.0 / d)));
                let fall = 1.0 - d / l.radius;
                let lambert = (v.normal.dot(to) / d).max(0.0);
                let got = l.color * (fall * fall * lambert * beam);
                color = color + got;
                dir = dir + to * (1.0 / d) * (got.x + got.y + got.z);
            }
            VertexLight {
                color: color * 256.0,
                dir: dir.normalized().unwrap_or(v.normal),
            }
        })
        .collect()
}

/// The voxel surfaces of a level with the light `lightmap` holds for them.
pub(super) fn lit_voxels(
    world: VoxelWorld,
    leaves: usize,
    sites: &CellSites,
    lightmap: &Lightmap,
) -> VoxelScene {
    VoxelScene::new(world, leaves, |chunk, mesh| {
        vertex_light(sites, lightmap.cells(), chunk, mesh)
    })
}

/// A compiled level waiting for its lightmap.
pub struct Compiled {
    parts: Parts,
    light_faces: Vec<LightFace>,
    bake_cfg: BakeConfig,
    /// [`shape_key`] of the volumes, which the light key holds.
    voxel_shape: u64,
}

impl Compiled {
    /// Runs CSG, face extraction and portals. No light is traced.
    pub fn new(level: &Level, cfg: &BuildConfig) -> Result<Self, BuildError> {
        let brushes = level.csg_brushes();
        let bsp = compile(&brushes).map_err(BuildError::Csg)?;
        let mut faces = extract_faces(&bsp, &brushes);
        // The voxel surface closes in front of a shell, before any light is laid out for it.
        faces.retain(|f| f.material != NO_DRAW);
        let holes = take_viewport_faces(&mut faces);
        let voxels = VoxelWorld::build(level, &level.static_brushes(), &bsp);
        let mover_ranges = append_mover_faces(level, &mut faces);
        let portals = Portals::build(&bsp);
        let vistas = Vistas::new(level, &bsp, &portals);
        let mut light_faces: Vec<LightFace> = faces.iter().map(LightFace::from).collect();
        vistas.set_densities(&faces, &mut light_faces);
        let lights = lights_of(level, cfg.light_gain);
        let play_lights = vistas.play_lights(&bsp, &lights);
        // The images land in the layers' rooms, so they stay out of `play_lights`.
        let lights = vistas.with_carried(&bsp, &holes, lights);
        let bake_cfg = BakeConfig {
            luxel_size: cfg.luxel_size,
            ambient: cfg.ambient,
            bounce_gain: 0.08,
            light_radius: cfg.light_radius,
            shadow_samples: cfg.shadow_samples,
            ..BakeConfig::default()
        };
        let voxel_shape = shape_key(&voxels);
        let sites = CellSites::of(&voxels);
        let parts = Parts {
            bsp,
            faces,
            holes,
            vistas,
            mover_ranges,
            portals,
            lights,
            play_lights,
            actors: actors_of(level),
            designs: designs_of(level)?,
            spawns: spawns_of(level),
            cfg: with_level_fog(cfg, level),
            voxels,
            sites,
        };
        Ok(Self {
            parts,
            light_faces,
            bake_cfg,
            voxel_shape,
        })
    }

    /// Identifies the full lightmap of this level: two levels with the same
    /// key can share one. It holds the shape of the voxel surfaces and not
    /// their materials, so painting a volume keeps it.
    pub fn light_key(&self) -> u64 {
        input_key(
            &self.light_faces,
            &self.parts.lights,
            &self.bake_cfg,
            self.voxel_shape,
        )
    }

    /// The full lightmap, if `cache` holds it in memory or in its file.
    pub fn stored(&self, cache: &mut BakeCache) -> Option<Arc<Lightmap>> {
        cache.lookup(
            self.light_key(),
            self.light_faces.len(),
            self.parts.sites.len(),
        )
    }

    pub(super) fn get_or_bake(&self, cache: &mut BakeCache) -> Result<Arc<Lightmap>, BuildError> {
        cache
            .get_or_bake(
                self.light_key(),
                (self.light_faces.len(), self.parts.sites.len()),
                self.bake_cfg.luxel_size,
                || self.bake_with(&self.bake_cfg, &AtomicBool::new(false)),
            )
            .map_err(BuildError::Bake)
    }

    /// Bakes with `cfg`: brush faces and voxel cells, each shadowing the other.
    fn bake_with(&self, cfg: &BakeConfig, cancel: &AtomicBool) -> Result<Lightmap, LightmapError> {
        let occ = Occluders::new(&self.parts.bsp, &self.parts.voxels);
        let (faces, lights) = (&self.light_faces, &self.parts.lights);
        bake_in(&occ, &self.parts.sites, faces, lights, cfg, cancel)
    }

    /// A coarse lightmap in milliseconds: large luxels, hard shadows, no
    /// bounce. The same lamps light the same surfaces, so the picture is right
    /// about where light falls until the full bake replaces it.
    pub fn draft(&self) -> Result<Lightmap, BuildError> {
        let cfg = BakeConfig {
            luxel_size: DRAFT_LUXEL.max(self.bake_cfg.luxel_size),
            light_radius: 0.0,
            shadow_samples: 1,
            bounce_gain: 0.0,
            ..self.bake_cfg
        };
        self.bake_with(&cfg, &AtomicBool::new(false))
            .map_err(BuildError::Bake)
    }

    /// The full bake on `workers` threads, touching no cache. Setting `cancel`
    /// ends it early with [`LightmapError::Cancelled`].
    pub fn bake(&self, workers: usize, cancel: &AtomicBool) -> Result<BakedLight, BuildError> {
        let cfg = BakeConfig {
            workers,
            ..self.bake_cfg
        };
        let t = Instant::now();
        let map = self.bake_with(&cfg, cancel).map_err(BuildError::Bake)?;
        Ok(BakedLight {
            key: self.light_key(),
            map,
            ms: t.elapsed().as_secs_f64() * 1e3,
            asked_luxel: cfg.luxel_size,
        })
    }

    /// The renderer of this level with `lightmap`: a draft, a bake or a stored
    /// one of the same faces.
    pub fn into_renderer(
        self,
        lightmap: Arc<Lightmap>,
        materials: MaterialTable,
    ) -> Result<LevelRenderer, BuildError> {
        check_materials(&materials)?;
        Ok(LevelRenderer::from_assets(LevelAssets::assemble(
            self.parts, lightmap, materials,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Quality, Source, View};
    use crate::{Framebuffer, MaterialTable};
    use mscent_math::Vec3;
    use mscent_world::lightmap::LightmapError;
    use mscent_world::showcase_level;
    use std::path::Path;
    use std::sync::atomic::Ordering;

    fn materials() -> MaterialTable {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        MaterialTable::load_dir(&dir).expect("textures")
    }

    fn compiled() -> Compiled {
        Compiled::new(&showcase_level(), &BuildConfig::default()).expect("compiles")
    }

    fn picture(r: &LevelRenderer) -> Vec<u32> {
        let mut fb = Framebuffer::new(160, 100).expect("fb");
        let view = View::from_angles(Vec3::new(0.0, 0.0, 2.0), 0.4, -0.1, 0.0);
        r.render(&mut fb, &view, Quality::Low, r.actors(), None)
            .expect("renders");
        fb.pixels().to_vec()
    }

    #[test]
    fn a_level_without_vistas_or_skies_keeps_its_key_and_its_lamps() {
        let c = compiled();
        let full: Vec<LightFace> = c.parts.faces.iter().map(LightFace::from).collect();
        let key = input_key(&full, &c.parts.lights, &c.bake_cfg, c.voxel_shape);
        assert_eq!(c.light_key(), key, "no density reaches the key");
        assert!(!c.parts.lights.is_empty());
        assert_eq!(c.parts.play_lights, c.parts.lights);
    }

    #[test]
    fn a_draft_lights_every_face_with_a_far_smaller_atlas_than_the_bake() {
        let c = compiled();
        let draft = c.draft().expect("drafts");
        let full = c.bake(2, &AtomicBool::new(false)).expect("bakes").map;
        assert_eq!(draft.projections().len(), full.projections().len());
        let area = |m: &Lightmap| m.atlas().width() as u64 * m.atlas().height() as u64;
        assert!(
            area(&draft) * 4 < area(&full),
            "draft {} full {}",
            area(&draft),
            area(&full)
        );
    }

    #[test]
    fn a_renderer_from_a_draft_draws_lit_textured_pixels() {
        let c = compiled();
        let map = Arc::new(c.draft().expect("drafts"));
        let r = c.into_renderer(map, materials()).expect("assembles");
        let colours: std::collections::HashSet<u32> = picture(&r).into_iter().collect();
        assert!(colours.len() > 50, "{} colours", colours.len());
    }

    #[test]
    fn a_bake_stored_in_the_cache_is_found_without_baking_again() {
        let mut cache = BakeCache::new();
        let c = compiled();
        assert!(c.stored(&mut cache).is_none(), "nothing held yet");
        let baked = c.bake(2, &AtomicBool::new(false)).expect("bakes");
        let held = cache.store(baked);
        assert_eq!(cache.bakes(), 1);
        let again = compiled();
        let found = again.stored(&mut cache).expect("found");
        assert!(Arc::ptr_eq(&held, &found), "the same lightmap is shared");
        assert_eq!(cache.last_source(), Some(Source::Memory));
        assert_eq!(cache.bakes(), 1);
    }

    #[test]
    fn the_stages_draw_exactly_what_the_one_call_path_draws() {
        let level = showcase_level();
        let cfg = BuildConfig::default();
        let direct = LevelRenderer::from_level(&level, materials(), &cfg).expect("builds");
        let c = Compiled::new(&level, &cfg).expect("compiles");
        let map = Arc::new(c.bake(2, &AtomicBool::new(false)).expect("bakes").map);
        let staged = c.into_renderer(map, materials()).expect("assembles");
        assert_eq!(picture(&direct), picture(&staged));
    }

    #[test]
    fn a_bake_cancelled_while_it_runs_stops_early() {
        let c = compiled();
        let t = Instant::now();
        c.bake(2, &AtomicBool::new(false)).expect("bakes");
        let whole = t.elapsed();
        let cancel = AtomicBool::new(false);
        let t = Instant::now();
        let r = std::thread::scope(|s| {
            let job = s.spawn(|| c.bake(2, &cancel));
            cancel.store(true, Ordering::Relaxed);
            job.join().expect("no panic")
        });
        let stopped = t.elapsed();
        println!("whole bake {whole:?}, cancelled one {stopped:?}");
        assert!(matches!(
            r.err(),
            Some(BuildError::Bake(LightmapError::Cancelled))
        ));
        assert!(stopped < whole / 2, "{stopped:?} of {whole:?}");
    }

    #[test]
    fn a_cancelled_bake_stops_and_says_so() {
        let cancel = AtomicBool::new(true);
        let r = compiled().bake(2, &cancel);
        assert!(matches!(
            r.err(),
            Some(BuildError::Bake(LightmapError::Cancelled))
        ));
        cancel.store(false, Ordering::Relaxed);
        assert!(compiled().bake(2, &cancel).is_ok());
    }
}
