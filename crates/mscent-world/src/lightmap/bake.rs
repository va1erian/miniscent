//! The baker: direct light with BSP shadow rays, then bounce and ambient.

use super::atlas::{Atlas, PADDING, Rect, ppm};
use super::bounce;
use super::parallel::par_map;
use super::ray::segment_clear;
use super::uv::{FaceProjection, project_face};
use super::{BakeConfig, LightFace, LightmapError, Luxel, PointLight};
use crate::bsp::Bsp;
use mscent_math::Vec3;

/// Everything a worker needs, shared read-only across threads.
pub(super) struct Scene<'a> {
    pub bsp: &'a Bsp,
    pub faces: &'a [LightFace],
    pub proj: &'a [FaceProjection],
    pub lights: &'a [PointLight],
    pub cfg: &'a BakeConfig,
}

/// A baked atlas of luxels plus the per-face frames needed to use it.
#[derive(Clone, Debug)]
pub struct Lightmap {
    atlas: Atlas,
    proj: Vec<FaceProjection>,
    texels: Vec<Luxel>,
}

impl Lightmap {
    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// Per-face UV frames, indexed like the faces given to [`bake`].
    pub fn projections(&self) -> &[FaceProjection] {
        &self.proj
    }

    /// Luxel at an absolute atlas position (gutter included).
    pub fn texel(&self, x: u32, y: u32) -> Option<Luxel> {
        if x >= self.atlas.width() || y >= self.atlas.height() {
            return None;
        }
        let i = y as usize * self.atlas.width() as usize + x as usize;
        self.texels.get(i).copied()
    }

    /// Luxel `(x, y)` of face `face`, in the face's own grid.
    pub fn face_luxel(&self, face: usize, x: u32, y: u32) -> Option<Luxel> {
        let r = self.atlas.rects().get(face)?;
        if x >= r.w || y >= r.h {
            return None;
        }
        self.texel(r.x + x, r.y + y)
    }

    /// Nearest luxel of `face` to a world point on it.
    pub fn sample_world(&self, face: usize, p: Vec3) -> Option<Luxel> {
        let (x, y) = self.proj.get(face)?.nearest_luxel(p);
        self.face_luxel(face, x, y)
    }

    /// Debug dump of the baked colours as a binary PPM (clamped, gamma 2).
    pub fn color_ppm(&self) -> Vec<u8> {
        let to_byte = |c: f32| (c.clamp(0.0, 1.0).sqrt() * 255.0 + 0.5) as u8;
        let px = self
            .texels
            .iter()
            .map(|l| [to_byte(l.color.x), to_byte(l.color.y), to_byte(l.color.z)]);
        ppm(
            self.atlas.width() as usize,
            self.atlas.height() as usize,
            px,
        )
    }
}

/// Bakes `faces` lit by `lights`, with shadows traced through `bsp`.
pub fn bake(
    bsp: &Bsp,
    faces: &[LightFace],
    lights: &[PointLight],
    cfg: &BakeConfig,
) -> Result<Lightmap, LightmapError> {
    validate(lights, cfg)?;
    let proj = faces
        .iter()
        .enumerate()
        .map(|(i, f)| project_face(f, i, cfg.luxel_size))
        .collect::<Result<Vec<_>, _>>()?;
    let sizes: Vec<(u32, u32)> = proj.iter().map(|p| (p.width, p.height)).collect();
    let atlas = Atlas::pack(&sizes, cfg.atlas_width, cfg.atlas_max_height)?;
    let scene = Scene {
        bsp,
        faces,
        proj: &proj,
        lights,
        cfg,
    };
    let rows = row_index(&proj);
    let direct = par_map(cfg.workers, rows.len(), |i| {
        direct_row(&scene, rows[i].0, rows[i].1)
    });
    let mut grids = rows_to_grids(&proj, direct);
    bounce::add_bounce(&scene, &rows, &mut grids);
    for grid in &mut grids {
        for l in grid {
            *l = finalize(*l, cfg.ambient);
        }
    }
    let texels = compose(&atlas, &grids);
    Ok(Lightmap {
        atlas,
        proj,
        texels,
    })
}

fn validate(lights: &[PointLight], cfg: &BakeConfig) -> Result<(), LightmapError> {
    let ok = cfg.luxel_size.is_finite()
        && cfg.luxel_size > 0.0
        && cfg.normal_bias.is_finite()
        && cfg.normal_bias >= 0.0
        && cfg.ambient.is_finite()
        && (0.0..=1.0).contains(&cfg.albedo)
        && cfg.bounce_gain.is_finite()
        && cfg.bounce_gain >= 0.0;
    if !ok {
        return Err(LightmapError::InvalidConfig);
    }
    for (i, l) in lights.iter().enumerate() {
        if !(l.position.is_finite() && l.color.is_finite() && l.radius.is_finite())
            || l.radius <= 0.0
        {
            return Err(LightmapError::InvalidLight(i));
        }
    }
    Ok(())
}

/// Work units: one per luxel row, face-major, so results stay in grid order.
pub(super) fn row_index(proj: &[FaceProjection]) -> Vec<(usize, u32)> {
    proj.iter()
        .enumerate()
        .flat_map(|(f, p)| (0..p.height).map(move |y| (f, y)))
        .collect()
}

/// Reassembles row results (in `row_index` order) into one grid per face.
pub(super) fn rows_to_grids(proj: &[FaceProjection], rows: Vec<Vec<Luxel>>) -> Vec<Vec<Luxel>> {
    let mut grids: Vec<Vec<Luxel>> = proj
        .iter()
        .map(|p| Vec::with_capacity(p.width as usize * p.height as usize))
        .collect();
    let faces = proj
        .iter()
        .enumerate()
        .flat_map(|(f, p)| std::iter::repeat_n(f, p.height as usize));
    for (f, row) in faces.zip(rows) {
        grids[f].extend(row);
    }
    grids
}

/// Rec. 709 luminance, used to weight directions by how much light they carry.
pub(super) fn luminance(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

fn direct_row(s: &Scene, face: usize, row: u32) -> Vec<Luxel> {
    let p = &s.proj[face];
    (0..p.width)
        .map(|x| direct_at(s, p, p.luxel_center(x, row)))
        .collect()
}

/// Accumulates colour and a luminance-weighted (not yet unit) direction.
fn direct_at(s: &Scene, p: &FaceProjection, point: Vec3) -> Luxel {
    let mut acc = Luxel {
        color: Vec3::ZERO,
        dir: Vec3::ZERO,
    };
    for light in s.lights {
        if let Some((c, to_light)) = light_at(s, p, point, light) {
            acc.color = acc.color + c;
            acc.dir = acc.dir + p.to_tangent(to_light) * luminance(c);
        }
    }
    acc
}

/// Light arriving at `point` from one lamp and the unit direction towards it,
/// or `None` when out of range, facing away or shadowed.
fn light_at(
    s: &Scene,
    p: &FaceProjection,
    point: Vec3,
    light: &PointLight,
) -> Option<(Vec3, Vec3)> {
    let to = light.position - point;
    let dist = to.length();
    if dist >= light.radius {
        return None;
    }
    let dir = to.normalized()?;
    let facing = dir.dot(p.normal);
    if facing <= 0.0 {
        return None;
    }
    // Start the ray off the surface so a face does not shadow itself.
    let origin = point + p.normal * s.cfg.normal_bias;
    if !segment_clear(s.bsp, origin.to_f64(), light.position.to_f64()) {
        return None;
    }
    let falloff = 1.0 - dist / light.radius;
    Some((light.color * (falloff * falloff * facing), dir))
}

/// Adds ambient and turns the accumulated direction into a unit vector, with
/// a defined fallback where no light arrived.
fn finalize(l: Luxel, ambient: Vec3) -> Luxel {
    Luxel {
        color: l.color + ambient,
        dir: l.dir.normalized().unwrap_or(Luxel::FALLBACK_DIR),
    }
}

/// Copies each face grid into its atlas rectangle and replicates the edge
/// luxels into the gutter.
fn compose(atlas: &Atlas, grids: &[Vec<Luxel>]) -> Vec<Luxel> {
    let aw = atlas.width() as usize;
    let mut texels = vec![Luxel::DARK; aw * atlas.height() as usize];
    for (r, grid) in atlas.rects().iter().zip(grids) {
        for (y, row) in grid.chunks(r.w as usize).enumerate() {
            let start = (r.y as usize + y) * aw + r.x as usize;
            texels[start..start + row.len()].copy_from_slice(row);
        }
        fill_gutter(&mut texels, aw, r);
    }
    texels
}

fn fill_gutter(texels: &mut [Luxel], aw: usize, r: &Rect) {
    for y in r.y - PADDING..r.y + r.h + PADDING {
        for x in r.x - PADDING..r.x + r.w + PADDING {
            let inside = x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h;
            if !inside {
                let sx = x.clamp(r.x, r.x + r.w - 1) as usize;
                let sy = y.clamp(r.y, r.y + r.h - 1) as usize;
                texels[y as usize * aw + x as usize] = texels[sy * aw + sx];
            }
        }
    }
}
