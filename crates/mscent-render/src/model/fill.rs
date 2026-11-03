//! Filling model triangles into the rows a band holds. A corner's position is in
//! model units, so the shading and the texture of the voxel path need to know
//! nothing of where the model is.

use mscent_math::Vec3;

use super::list::{GLOW_LIGHT, ListedPart, ModelList};
use super::{Level, ModelTable};
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::DrawStats;
use crate::shade::Fog;
use crate::specular::SpecTable;
use crate::texture::Texture;
use crate::voxel::fill::{FillContext, draw_look};
use crate::voxel::list::ScreenV;
use crate::voxel::tri::{Corner, clip_near_of};

/// Everything a frame's model triangles share.
pub(crate) struct ModelFill<'a> {
    pub cam: &'a Camera,
    pub fog: Option<Fog>,
    pub bump: bool,
    pub specular: bool,
    /// One per material id, then one for unknown ids.
    pub spec: &'a [SpecTable],
    pub texture_of: &'a (dyn Fn(u16) -> &'a Texture + Sync),
}

/// Fills every listed part touching the rows `fb` holds.
pub(crate) fn fill_models(
    fb: &mut Framebuffer,
    list: &ModelList,
    table: &ModelTable,
    fill: &ModelFill,
) -> DrawStats {
    let mut stats = DrawStats::default();
    let band = fb.row_range();
    for listed in &list.parts {
        if listed.first >= band.1 || listed.last <= band.0 {
            continue;
        }
        if let Some(level) = table.level_of(listed) {
            fill_part(fb, list, listed, level, fill, band, &mut stats);
        }
    }
    stats
}

fn context<'a>(fill: &ModelFill<'a>, eye: Vec3, lit: bool) -> FillContext<'a> {
    FillContext {
        cam: fill.cam,
        eye,
        fog: fill.fog,
        bump: fill.bump && lit,
        specular: fill.specular && lit,
        lights: &[],
        spec: fill.spec,
        texture_of: fill.texture_of,
    }
}

fn fill_part(
    fb: &mut Framebuffer,
    list: &ModelList,
    listed: &ListedPart,
    level: &Level,
    fill: &ModelFill,
    band: (i32, i32),
    stats: &mut DrawStats,
) {
    let count = level.vertices.len();
    let end = listed.start + count;
    let (Some(screen), Some(light)) = (
        list.screen.get(listed.start..end),
        list.light.get(listed.start..end),
    ) else {
        return;
    };
    let lit = context(fill, listed.eye, true);
    let glowing = context(fill, listed.eye, false);
    let row16 = (band.0.saturating_mul(16), band.1.saturating_mul(16));
    for (t, &glow) in level.triangles.iter().zip(&level.glow) {
        let index = t.map(usize::from);
        let [a, b, d] = index.map(|i| screen[i]);
        if faces_away_or_off_band([a, b, d], row16) {
            continue;
        }
        let c = corners(level, listed, light, index, [a, b, d], glow);
        let ctx = if glow { &glowing } else { &lit };
        let look = (level.axis.as_slice(), level.material.as_slice(), index);
        if c.iter().all(|k| k.iz > 0.0) {
            draw_look(fb, &c, look, ctx, stats);
            continue;
        }
        let to_world = |p: Vec3| listed.place.point(p);
        for piece in clip_near_of(fill.cam, &c, to_world).into_iter().flatten() {
            draw_look(fb, &piece, look, ctx, stats);
        }
    }
}

/// A cheap first cut. A triangle crossing the near plane can land anywhere, so it
/// goes on.
fn faces_away_or_off_band(v: [ScreenV; 3], row16: (i32, i32)) -> bool {
    let [a, b, d] = v;
    if a.iz <= 0.0 || b.iz <= 0.0 || d.iz <= 0.0 {
        return false;
    }
    let (lo, hi) = (a.y.min(b.y).min(d.y), a.y.max(b.y).max(d.y));
    if hi < row16.0 || lo >= row16.1 {
        return true;
    }
    let cross =
        i64::from(b.x - a.x) * i64::from(d.y - a.y) - i64::from(d.x - a.x) * i64::from(b.y - a.y);
    cross >= 0
}

fn corners(
    level: &Level,
    listed: &ListedPart,
    light: &[Vec3],
    index: [usize; 3],
    screen: [ScreenV; 3],
    glow: bool,
) -> [Corner; 3] {
    [0, 1, 2].map(|k| {
        let i = index[k];
        let v = &level.vertices[i];
        let mut weight = Vec3::ZERO;
        match k {
            0 => weight.x = 1.0,
            1 => weight.y = 1.0,
            _ => weight.z = 1.0,
        }
        Corner {
            x: screen[k].x,
            y: screen[k].y,
            iz: screen[k].iz,
            pos: v.pos,
            light: if glow { GLOW_LIGHT } else { light[i] },
            dir: listed.tangent_dir(v.normal),
            world_dir: listed.dir,
            normal: v.normal,
            weight,
        }
    })
}
