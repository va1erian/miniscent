//! The baker: direct light with BSP shadow rays, then bounce and ambient.

use super::atlas::{Atlas, PADDING, Rect, ppm};
use super::bounce;
use super::cell_sites::CellSites;
use super::cells::{CellLighting, CellRecord, direct_pass, settle};
use super::cells_bounce;
use super::filter;
use super::layout::{Layout, lay_out};
use super::occluders::Occluders;
use super::parallel::par_map;
use super::soft::{self, Footprint, each_light};
use super::uv::FaceProjection;
use super::{BakeConfig, LightFace, LightmapError, Luxel, PointLight};
use crate::bsp::Bsp;
use mscent_math::Vec3;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cap on shadow rays per luxel and light, so a config cannot make a bake
/// run for hours.
pub const MAX_SHADOW_SAMPLES: u32 = 256;

/// Everything a worker needs, shared read-only across threads.
pub(super) struct Scene<'a> {
    pub occ: &'a Occluders<'a>,
    pub faces: &'a [LightFace],
    pub proj: &'a [FaceProjection],
    pub lights: &'a [PointLight],
    pub cfg: &'a BakeConfig,
    /// Set from outside to abandon the bake.
    pub cancel: &'a AtomicBool,
}

impl Scene<'_> {
    /// Whether the bake was abandoned; workers then skip their remaining rows.
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// A baked atlas of luxels plus the per-face frames needed to use it.
#[derive(Clone, Debug)]
pub struct Lightmap {
    atlas: Atlas,
    proj: Vec<FaceProjection>,
    texels: Vec<Luxel>,
    /// Light of the surface cells of voxel volumes, in the order of the
    /// [`CellSites`] the bake was given.
    cells: Vec<CellRecord>,
}

impl Lightmap {
    pub(super) fn from_parts(
        atlas: Atlas,
        proj: Vec<FaceProjection>,
        texels: Vec<Luxel>,
        cells: Vec<CellRecord>,
    ) -> Self {
        Self {
            atlas,
            proj,
            texels,
            cells,
        }
    }

    /// The light of each voxel surface cell, in site order.
    pub fn cells(&self) -> &[CellRecord] {
        &self.cells
    }

    /// All luxels, atlas row by row (gutter included).
    pub(super) fn texel_slice(&self) -> &[Luxel] {
        &self.texels
    }

    pub fn atlas(&self) -> &Atlas {
        &self.atlas
    }

    /// Per-face UV frames, indexed like the faces given to [`bake`].
    pub fn projections(&self) -> &[FaceProjection] {
        &self.proj
    }

    /// The finest luxel size of any face, in world units: the size asked for
    /// unless the level had to be coarsened to fit the atlas.
    pub fn finest_luxel(&self) -> Option<f32> {
        let lit = self.proj.iter().filter(|p| p.width > 0 && p.height > 0);
        let sizes = lit.map(|p| p.luxel_size);
        sizes.reduce(f32::min)
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

    /// Direct view of one face's luxels for per-pixel lookups, or `None` for
    /// an unknown face and for one with no luxels.
    pub fn face_grid(&self, face: usize) -> Option<FaceGrid<'_>> {
        let r = self
            .atlas
            .rects()
            .get(face)
            .filter(|r| **r != Rect::EMPTY)?;
        let stride = self.atlas.width() as usize;
        Some(FaceGrid {
            texels: &self.texels,
            start: r.y as usize * stride + r.x as usize,
            stride,
            w: r.w,
            h: r.h,
        })
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

/// One face's luxel grid inside the atlas, with its row stride.
#[derive(Clone, Copy, Debug)]
pub struct FaceGrid<'a> {
    texels: &'a [Luxel],
    start: usize,
    stride: usize,
    w: u32,
    h: u32,
}

impl FaceGrid<'_> {
    pub fn width(&self) -> u32 {
        self.w
    }

    pub fn height(&self) -> u32 {
        self.h
    }

    /// Luxel `(x, y)` with both clamped into the face's own grid, so
    /// filtering never reads a neighbour's luxels.
    #[inline]
    pub fn at_clamped(&self, x: u32, y: u32) -> Luxel {
        let x = x.min(self.w.saturating_sub(1)) as usize;
        let y = y.min(self.h.saturating_sub(1)) as usize;
        self.texels
            .get(self.start + y * self.stride + x)
            .copied()
            .unwrap_or(Luxel::DARK)
    }
}

/// Bakes `faces` lit by `lights`, with shadows traced through `bsp`.
pub fn bake(
    bsp: &Bsp,
    faces: &[LightFace],
    lights: &[PointLight],
    cfg: &BakeConfig,
) -> Result<Lightmap, LightmapError> {
    bake_cancellable(bsp, faces, lights, cfg, &AtomicBool::new(false))
}

/// Like [`bake`], but setting `cancel` makes it return
/// [`LightmapError::Cancelled`] within one luxel row per worker, so an editor
/// can drop a bake whose level has changed.
pub fn bake_cancellable(
    bsp: &Bsp,
    faces: &[LightFace],
    lights: &[PointLight],
    cfg: &BakeConfig,
    cancel: &AtomicBool,
) -> Result<Lightmap, LightmapError> {
    let occ = Occluders::brushes(bsp);
    bake_in(&occ, &CellSites::default(), faces, lights, cfg, cancel)
}

/// Like [`bake_cancellable`], with shadows from `occ`, which may include voxel
/// volumes, and a record of light for each of `sites`.
pub fn bake_in(
    occ: &Occluders<'_>,
    sites: &CellSites,
    faces: &[LightFace],
    lights: &[PointLight],
    cfg: &BakeConfig,
    cancel: &AtomicBool,
) -> Result<Lightmap, LightmapError> {
    validate(lights, cfg)?;
    let Layout { atlas, proj } = lay_out(faces, cfg)?;
    let scene = Scene {
        occ,
        faces,
        proj: &proj,
        lights,
        cfg,
        cancel,
    };
    let rows = row_index(&proj);
    let direct = par_map(cfg.workers, rows.len(), |i| {
        if scene.cancelled() {
            return Vec::new();
        }
        direct_row(&scene, rows[i].0, rows[i].1)
    });
    if scene.cancelled() {
        return Err(LightmapError::Cancelled);
    }
    let mut grids = rows_to_grids(&proj, direct);
    // Hard shadows (one ray, no lamp radius) stay exact; soft ones are smoothed.
    if cfg.light_radius > 0.0 && cfg.shadow_samples > 1 {
        for (grid, p) in grids.iter_mut().zip(&proj) {
            filter::smooth(grid, p.width as usize, p.height as usize);
        }
    }
    let patches = bounce::face_patches(&scene, &grids);
    let env = CellLighting { occ, lights, cfg };
    let which: Vec<usize> = (0..sites.len()).collect();
    let mut direct = direct_pass(&env, sites, &which, cancel);
    if scene.cancelled() {
        return Err(LightmapError::Cancelled);
    }
    bounce::add_bounce(&scene, &rows, &patches, &mut grids);
    cells_bounce::add_bounce(&scene, sites, &patches, &mut direct);
    if scene.cancelled() {
        return Err(LightmapError::Cancelled);
    }
    for grid in &mut grids {
        for l in grid {
            *l = finalize(*l, cfg.ambient);
        }
    }
    let texels = compose(&atlas, &grids);
    let cells = settle(&env, sites, &which, &direct);
    Ok(Lightmap::from_parts(atlas, proj, texels, cells))
}

fn validate(lights: &[PointLight], cfg: &BakeConfig) -> Result<(), LightmapError> {
    let ok = cfg.luxel_size.is_finite()
        && cfg.luxel_size > 0.0
        && cfg.normal_bias.is_finite()
        && cfg.normal_bias >= 0.0
        && cfg.light_radius.is_finite()
        && cfg.light_radius >= 0.0
        && cfg.shadow_samples <= MAX_SHADOW_SAMPLES
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
        .map(|x| {
            let verts = &s.faces[face].vertices;
            let centre = p.luxel_sample(verts, x, row);
            let rx = soft::Receiver {
                occ: s.occ,
                foot: Footprint::Luxel {
                    proj: p,
                    verts,
                    x,
                    y: row,
                },
                bias: s.cfg.normal_bias,
                centre,
            };
            direct_at(s, &rx, p)
        })
        .collect()
}

/// Accumulates colour and a luminance-weighted (not yet unit) direction, in the
/// tangent frame of `proj`.
fn direct_at(s: &Scene, rx: &soft::Receiver, proj: &FaceProjection) -> Luxel {
    let mut acc = Luxel {
        color: Vec3::ZERO,
        dir: Vec3::ZERO,
    };
    each_light(rx, s.lights, s.cfg, |c, to_light| {
        acc.color = acc.color + c;
        acc.dir = acc.dir + proj.to_tangent(to_light) * luminance(c);
    });
    acc
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
        if *r == Rect::EMPTY {
            continue;
        }
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
