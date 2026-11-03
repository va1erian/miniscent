//! What the face shader reads about each face and material, as the bytes of its
//! storage buffers. Which shading a face gets is `face_shader` in `level/fill.rs`.

use std::collections::HashMap;

use mscent_render::scene::Scene;
use mscent_render::specular::SpecTable;
use mscent_render::{EMISSIVE, LevelAssets, SELF_LIT, UNLIT};
use mscent_world::materials::{LIGHTPANEL, is_self_lit};

use crate::lightmap::{FaceRects, LightAtlas};

const HAS_NORMALS: u32 = 1;
const SPEC_VISIBLE: u32 = 2;
/// A vista layer's pan slides its texture.
const PANS: u32 = 4;
/// Entries of a [`SpecTable`], and the row of them a material gets.
pub(crate) const TABLE_LEN: usize = 256;

fn push_floats(out: &mut Vec<u8>, values: &[f32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

fn push_uints(out: &mut Vec<u8>, values: &[u32]) {
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// The light of a face drawn flat; `None` for one the lightmap lights.
fn flat_light(
    assets: &LevelAssets,
    index: usize,
    material: u16,
    rects: Option<&FaceRects>,
) -> Option<u32> {
    if material == LIGHTPANEL {
        return Some(EMISSIVE);
    }
    if is_self_lit(material) {
        return Some(SELF_LIT);
    }
    let lit = assets.uv_map(index).is_some() && rects.is_some_and(|r| !r.levels.is_empty());
    (!lit).then_some(UNLIT)
}

/// The `Face` records, the `levels` they point into, and the table rows.
pub(crate) struct FaceData {
    pub faces: Vec<u8>,
    pub levels: Vec<u8>,
    pub tables: Vec<u8>,
}

/// One table's entries, read back through its own interpolation.
fn push_table(tables: &mut Vec<u8>, table: &SpecTable) {
    for i in 0..TABLE_LEN {
        let entry = table.response(i as f32 / (TABLE_LEN - 1) as f32);
        tables.extend_from_slice(&entry.to_le_bytes());
    }
}

pub(crate) fn build(assets: &LevelAssets, atlas: &LightAtlas) -> FaceData {
    let scene: &Scene = assets.scene();
    let mut out = FaceData {
        faces: Vec::new(),
        levels: Vec::new(),
        tables: Vec::new(),
    };
    // By table, its row: a level's unknown materials all share the last table,
    // so a row a material would make thousands of the same rows.
    let mut level_count = 0u32;
    let mut rows = HashMap::<Option<*const SpecTable>, usize>::new();
    for (index, face) in scene.faces().iter().enumerate() {
        let rects = atlas.faces.get(index);
        let texture = scene.texture_of(face.material);
        let table = assets.spec_table(face.material);
        let next = rows.len();
        let row = *rows
            .entry(table.map(std::ptr::from_ref))
            .or_insert_with(|| {
                if let Some(table) = table {
                    push_table(&mut out.tables, table);
                } else {
                    out.tables.resize(out.tables.len() + TABLE_LEN * 4, 0);
                }
                next
            });
        let flat = flat_light(assets, index, face.material, rects);
        let map = assets.uv_map(index);
        let m = map.map_or([0.0; 6], |m| m.coefficients());
        let per_texel = map.map_or(0.0, |m| m.luxels_per_texel());
        let frame = map.map_or([0.0; 4], |m| m.frame());
        let projection = assets.lightmap().projections().get(index);
        let v3 = |v: Option<mscent_math::Vec3>| v.map_or([0.0; 3], |v| [v.x, v.y, v.z]);
        let [ox, oy, oz] = v3(projection.map(|p| p.origin));
        let [tx, ty, tz] = v3(projection.map(|p| p.tangent));
        let [bx, by, bz] = v3(projection.map(|p| p.bitangent));
        let [nx, ny, nz] = v3(projection.map(|p| p.normal));
        let (luxel, u_min, v_min) =
            projection.map_or((0.0, 0.0, 0.0), |p| (p.luxel_size, p.u_min, p.v_min));
        let dir = rects.map_or([0.0; 4], |r| r.dir.as_vec4());
        push_floats(&mut out.faces, &[m[0], m[1], m[2], per_texel]);
        push_floats(
            &mut out.faces,
            &[m[3], m[4], m[5], flat.unwrap_or(0) as f32],
        );
        push_floats(&mut out.faces, &frame);
        push_floats(&mut out.faces, &[ox, oy, oz, luxel]);
        push_floats(&mut out.faces, &[tx, ty, tz, u_min]);
        push_floats(&mut out.faces, &[bx, by, bz, v_min]);
        push_floats(&mut out.faces, &[nx, ny, nz, texture.spec_mean()]);
        push_floats(&mut out.faces, &dir);
        let visible = table.is_some_and(SpecTable::is_visible);
        let flags = u32::from(texture.has_normals()) * HAS_NORMALS
            + u32::from(visible) * SPEC_VISIBLE
            + u32::from(is_self_lit(face.material)) * PANS;
        let count = rects.map_or(0, |r| r.levels.len() as u32);
        push_uints(
            &mut out.faces,
            &[level_count, count.max(1), flags, row as u32],
        );
        match rects.filter(|r| !r.levels.is_empty()) {
            Some(r) => {
                for rect in &r.levels {
                    push_floats(&mut out.levels, &rect.as_vec4());
                }
                level_count += count;
            }
            None => {
                push_floats(&mut out.levels, &[0.0; 4]);
                level_count += 1;
            }
        }
    }
    out
}
