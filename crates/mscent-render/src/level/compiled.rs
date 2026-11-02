//! A level between its two costs: CSG, faces and portals are done, the light
//! is not. [`LevelRenderer::from_level_cached`] goes straight through; an
//! editor stops here so the picture can follow every edit: it asks for a stored
//! lightmap, falls back to a draft that takes milliseconds, and leaves the full
//! bake to a thread that can be cancelled and holds no lock on the cache.

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use mscent_world::lightmap::store::input_key;
use mscent_world::lightmap::{BakeConfig, LightFace, Lightmap, PointLight, bake, bake_cancellable};
use mscent_world::{Bsp, Face, Level, Portals, brush_faces, compile, extract_faces};

use super::{
    Actor, BakeCache, BakedLight, BuildConfig, BuildError, LevelRenderer, actors_of,
    check_materials, lights_of, with_level_fog,
};
use crate::texture::MaterialTable;

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

/// What the renderer keeps of a compiled level.
pub(super) struct Parts {
    pub bsp: Bsp,
    /// The world's faces, then each mover's.
    pub faces: Vec<Face>,
    /// For each mover, which of `faces` are its own.
    pub mover_ranges: Vec<Range<usize>>,
    pub portals: Portals,
    pub lights: Vec<PointLight>,
    pub actors: Vec<Actor>,
    pub cfg: BuildConfig,
}

/// A compiled level waiting for its lightmap.
pub struct Compiled {
    parts: Parts,
    light_faces: Vec<LightFace>,
    bake_cfg: BakeConfig,
}

impl Compiled {
    /// Runs CSG, face extraction and portals. No light is traced.
    pub fn new(level: &Level, cfg: &BuildConfig) -> Result<Self, BuildError> {
        let brushes = level.static_brushes();
        let bsp = compile(&brushes).map_err(BuildError::Csg)?;
        let mut faces = extract_faces(&bsp, &brushes);
        let mover_ranges = append_mover_faces(level, &mut faces);
        let portals = Portals::build(&bsp);
        let light_faces = faces.iter().map(LightFace::from).collect();
        let bake_cfg = BakeConfig {
            luxel_size: cfg.luxel_size,
            ambient: cfg.ambient,
            bounce_gain: 0.08,
            light_radius: cfg.light_radius,
            shadow_samples: cfg.shadow_samples,
            ..BakeConfig::default()
        };
        let parts = Parts {
            bsp,
            faces,
            mover_ranges,
            portals,
            lights: lights_of(level, cfg.light_gain),
            actors: actors_of(level),
            cfg: with_level_fog(cfg, level),
        };
        Ok(Self {
            parts,
            light_faces,
            bake_cfg,
        })
    }

    /// Identifies the full lightmap of this level: two levels with the same
    /// key can share one.
    pub fn light_key(&self) -> u64 {
        input_key(&self.light_faces, &self.parts.lights, &self.bake_cfg)
    }

    /// The full lightmap, if `cache` holds it in memory or in its file.
    pub fn stored(&self, cache: &mut BakeCache) -> Option<Arc<Lightmap>> {
        cache.lookup(self.light_key(), self.light_faces.len())
    }

    pub(super) fn get_or_bake(&self, cache: &mut BakeCache) -> Result<Arc<Lightmap>, BuildError> {
        cache
            .get_or_bake(
                &self.parts.bsp,
                &self.light_faces,
                &self.parts.lights,
                &self.bake_cfg,
            )
            .map_err(BuildError::Bake)
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
        bake(&self.parts.bsp, &self.light_faces, &self.parts.lights, &cfg).map_err(BuildError::Bake)
    }

    /// The full bake on `workers` threads, touching no cache. Setting `cancel`
    /// ends it early with [`LightmapError::Cancelled`].
    pub fn bake(&self, workers: usize, cancel: &AtomicBool) -> Result<BakedLight, BuildError> {
        let cfg = BakeConfig {
            workers,
            ..self.bake_cfg
        };
        let t = Instant::now();
        let map = bake_cancellable(
            &self.parts.bsp,
            &self.light_faces,
            &self.parts.lights,
            &cfg,
            cancel,
        )
        .map_err(BuildError::Bake)?;
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
        Ok(LevelRenderer::assemble(self.parts, lightmap, materials))
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
