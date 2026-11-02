//! Picks the luxel size a level is baked at: the one asked for if the level
//! fits the atlas, otherwise the first coarser one that does. A level that is
//! too big thus gets softer light instead of none. Nothing is traced here, so
//! a retry costs microseconds per face.

use super::atlas::{Atlas, AtlasError};
use super::uv::{FaceProjection, project_face};
use super::{BakeConfig, LightFace, LightmapError};

/// Growth of the luxel size per attempt. Each step drops about a third of the
/// luxels, so a level loses little more detail than it has to.
const COARSEN: f32 = 1.25;

/// Where every face's luxels go.
pub(super) struct Layout {
    pub atlas: Atlas,
    /// Per-face frames, all at the chosen luxel size or coarser.
    pub proj: Vec<FaceProjection>,
}

/// Lays `faces` out at `cfg.luxel_size`, coarsening until they fit. Fails only
/// when one luxel per face is still too much for the atlas.
pub(super) fn lay_out(faces: &[LightFace], cfg: &BakeConfig) -> Result<Layout, LightmapError> {
    let mut size = cfg.luxel_size;
    loop {
        let proj = faces
            .iter()
            .enumerate()
            .map(|(i, f)| project_face(f, i, size))
            .collect::<Result<Vec<_>, _>>()?;
        let sizes: Vec<(u32, u32)> = proj.iter().map(|p| (p.width, p.height)).collect();
        match Atlas::pack(&sizes, cfg.atlas_width, cfg.atlas_max_height) {
            Ok(atlas) => return Ok(Layout { atlas, proj }),
            // Faces shrink to one luxel as `size` grows, so this ends.
            Err(AtlasError::TooTall | AtlasError::TooWide(_))
                if sizes.iter().any(|&s| s != (1, 1)) =>
            {
                size *= COARSEN;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::store::MAX_BYTES;
    use super::super::{Lightmap, bake};
    use super::*;
    use crate::{GenParams, compile, extract_faces, generate_level};
    use mscent_math::Vec3;

    const LUXEL_BYTES: u64 = 24;

    fn generated(cols: u32, rows: u32) -> Vec<LightFace> {
        let params = GenParams {
            cols,
            rows,
            ..GenParams::default()
        };
        let level = generate_level(7, &params);
        let bsp = compile(&level.brushes).expect("compiles");
        let faces = extract_faces(&bsp, &level.brushes);
        faces.iter().map(LightFace::from).collect()
    }

    fn at_game_luxels() -> BakeConfig {
        BakeConfig {
            luxel_size: 0.125,
            ..BakeConfig::default()
        }
    }

    fn finest(layout: &Layout) -> f32 {
        let sizes = layout.proj.iter().map(|p| p.luxel_size);
        sizes.fold(f32::MAX, f32::min)
    }

    fn square(at: f32, side: f32) -> LightFace {
        let p = |x: f32, y: f32| Vec3::new(at + x, y, 0.0);
        LightFace {
            vertices: vec![p(0.0, 0.0), p(side, 0.0), p(side, side), p(0.0, side)],
            normal: Vec3::new(0.0, 0.0, 1.0),
        }
    }

    #[test]
    fn the_largest_stored_atlas_fits_the_cache_file_limit() {
        let cfg = BakeConfig::default();
        let bytes = u64::from(cfg.atlas_width) * u64::from(cfg.atlas_max_height) * LUXEL_BYTES;
        // What is left over holds the header and the per-face records.
        assert!(bytes < MAX_BYTES as u64, "{bytes} bytes");
    }

    #[test]
    fn eight_by_eight_rooms_keep_the_luxel_size_they_asked_for() {
        let cfg = at_game_luxels();
        let layout = lay_out(&generated(8, 8), &cfg).expect("fits");
        println!("atlas height {}", layout.atlas.height());
        assert_eq!(finest(&layout), cfg.luxel_size);
        assert!(layout.atlas.height() > 4096, "needs more than the old cap");
    }

    #[test]
    fn a_level_past_the_atlas_limit_coarsens_only_as_far_as_it_must() {
        // The generator stops at 8 by 8, so the limit comes down instead.
        let cfg = BakeConfig {
            atlas_max_height: 4096,
            ..at_game_luxels()
        };
        let faces = generated(8, 8);
        let layout = lay_out(&faces, &cfg).expect("fits after coarsening");
        let size = finest(&layout);
        println!(
            "{} faces: luxel {size}, atlas height {}",
            faces.len(),
            layout.atlas.height()
        );
        assert!(size > cfg.luxel_size, "this level does not fit as asked");
        assert!(layout.atlas.height() <= cfg.atlas_max_height);
        let one_step_finer = BakeConfig {
            atlas_max_height: u32::MAX,
            luxel_size: size / COARSEN,
            ..cfg
        };
        let tighter = lay_out(&faces, &one_step_finer).expect("no limit");
        assert!(tighter.atlas.height() > cfg.atlas_max_height);
    }

    #[test]
    fn an_atlas_past_the_old_limit_bakes_and_reads_back_from_its_bytes() {
        let params = GenParams {
            cols: 8,
            rows: 8,
            ..GenParams::default()
        };
        let level = generate_level(7, &params);
        let bsp = compile(&level.brushes).expect("compiles");
        let found = extract_faces(&bsp, &level.brushes);
        let faces: Vec<LightFace> = found.iter().map(LightFace::from).collect();
        // No lamps and no bounce: tracing is not what this test is about, and
        // it would take minutes unoptimised.
        let cfg = BakeConfig {
            bounce_gain: 0.0,
            albedo: 0.0,
            ..at_game_luxels()
        };
        let map = bake(&bsp, &faces, &[], &cfg).expect("bakes");
        assert!(map.atlas().height() > 4096, "{}", map.atlas().height());
        let bytes = map.to_bytes(1);
        assert!(bytes.len() < MAX_BYTES, "{} bytes", bytes.len());
        let back = Lightmap::from_bytes(&bytes, 1).expect("reads back");
        assert_eq!(back.atlas(), map.atlas());
        assert_eq!(back.finest_luxel(), Some(cfg.luxel_size));
    }

    #[test]
    fn a_face_wider_than_the_atlas_coarsens_too() {
        let cfg = BakeConfig {
            luxel_size: 0.125,
            atlas_width: 34,
            atlas_max_height: 34,
            ..BakeConfig::default()
        };
        let layout = lay_out(&[square(0.0, 32.0)], &cfg).expect("fits");
        assert!(finest(&layout) >= 1.0, "{}", finest(&layout));
        assert!(layout.atlas.width() == 34 && layout.atlas.height() <= 34);
    }

    #[test]
    fn coarsening_gives_up_once_every_face_is_one_luxel() {
        let faces = [square(0.0, 8.0), square(10.0, 8.0), square(20.0, 8.0)];
        let cfg = |w, h| BakeConfig {
            luxel_size: 0.125,
            atlas_width: w,
            atlas_max_height: h,
            ..BakeConfig::default()
        };
        let err = |e| Some(LightmapError::Atlas(e));
        let tall = lay_out(&faces, &cfg(3, 6)).err();
        assert_eq!(tall, err(AtlasError::TooTall));
        let wide = lay_out(&faces, &cfg(2, 100)).err();
        assert_eq!(wide, err(AtlasError::TooWide(0)));
        assert!(lay_out(&faces, &cfg(3, 9)).is_ok(), "one luxel each fits");
    }

    #[test]
    fn a_broken_face_is_reported_not_coarsened_away() {
        let mut bad = square(0.0, 8.0);
        bad.vertices.truncate(2);
        let r = lay_out(&[bad], &at_game_luxels()).err();
        assert_eq!(r, Some(LightmapError::DegenerateFace(0)));
    }
}
