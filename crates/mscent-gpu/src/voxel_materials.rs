//! What the voxel shader reads about materials: every texture a voxel id can
//! name, with its mips and normal map, in one storage buffer of texels.
//!
//! One triangle can carry three materials (`voxel/blend.rs`), so a bind group per
//! material, as faces have, cannot draw it. A buffer holds any number of them with
//! no device feature, and the shader filters the software renderer's way, with its
//! integer weights, on the same box-filtered mips (`texture.rs`).

use mscent_render::specular::SpecTable;
use mscent_render::voxel::has_spots;
use mscent_render::{LevelAssets, Texture};

use crate::device::GpuError;
use crate::face_data::TABLE_LEN;

/// A voxel's material is a byte.
pub(crate) const MATERIALS: usize = 256;
/// Albedo levels a material may have: up to a 32768-texel side.
const MAX_LEVELS: usize = 16;
/// The words of a `Material` of `voxel.wgsl`.
const MATERIAL_WORDS: usize = 8 + MAX_LEVELS;
/// Where a material with no normal map points its normals.
const NO_NORMALS: u32 = u32::MAX;

/// A one-texel image at the start of the buffer, with no normals.
const PLACEHOLDER: [u32; MATERIAL_WORDS] = {
    let mut words = [0; MATERIAL_WORDS];
    words[0] = 1;
    words[1] = 1;
    words[3] = NO_NORMALS;
    words
};

const SMOOTH: u32 = 1;
const SPEC_VISIBLE: u32 = 2;
const SPOTS: u32 = 4;

/// The `materials`, `texels` and `spec_tables` buffers of `voxel.wgsl`.
pub(crate) struct VoxelMaterials {
    pub table: Vec<u8>,
    pub texels: Vec<u8>,
    pub tables: Vec<u8>,
}

/// A texture already in the buffer, and the words of its record.
struct Placed<'a> {
    texture: &'a Texture,
    words: [u32; MATERIAL_WORDS],
}

fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

/// The index the next texel pushed onto `texels` will have.
fn next_texel(texels: &[u8]) -> Result<u32, GpuError> {
    u32::try_from(texels.len() / 4).map_err(|_| GpuError::TooLarge("voxel textures"))
}

/// Appends `texture`'s levels and normals, and returns its record without the
/// per-material words.
fn place(texels: &mut Vec<u8>, texture: &Texture) -> Result<[u32; MATERIAL_WORDS], GpuError> {
    let (w, h) = (texture.width(), texture.height());
    let levels = texture.mip_count() as usize + 1;
    if levels > MAX_LEVELS {
        return Err(GpuError::TooLarge("a voxel texture's levels"));
    }
    let mut words = [0; MATERIAL_WORDS];
    words[0] = w;
    words[1] = h;
    words[2] = texture.mip_count();
    for (level, slot) in (0u32..).zip(&mut words[8..8 + levels]) {
        *slot = next_texel(texels)?;
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        for y in 0..lh as i32 {
            for x in 0..lw as i32 {
                push_u32(texels, texture.texel_lod(x << level, y << level, level));
            }
        }
    }
    words[3] = NO_NORMALS;
    if texture.has_normals() {
        words[3] = next_texel(texels)?;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                push_u32(texels, texture.normal_texel(x, y).unwrap_or(0));
            }
        }
    }
    Ok(words)
}

/// One power table's entries, read back through its own interpolation, as the
/// face data has them.
fn push_table(tables: &mut Vec<u8>, table: Option<&SpecTable>) {
    for i in 0..TABLE_LEN {
        let entry = table.map_or(0.0, |t| t.response(i as f32 / (TABLE_LEN - 1) as f32));
        tables.extend_from_slice(&entry.to_le_bytes());
    }
}

/// The table row of `table`, adding it on first sight.
fn spec_row<'a>(
    rows: &mut Vec<Option<&'a SpecTable>>,
    tables: &mut Vec<u8>,
    table: Option<&'a SpecTable>,
) -> u32 {
    let same = |r: &Option<&SpecTable>| match (r, table) {
        (Some(a), Some(b)) => std::ptr::eq(*a, b),
        (None, None) => true,
        _ => false,
    };
    let at = rows.iter().position(same).unwrap_or_else(|| {
        rows.push(table);
        push_table(tables, table);
        rows.len() - 1
    });
    at as u32
}

/// Every material id's record, with the texels of those `wanted`: the rest point
/// at one texel, and a voxel naming one of them must not be drawn until they are
/// built again with it. Ids with no material of their own share the scene's
/// fallback texture, so at most one texture per real material is stored.
pub(crate) fn build(
    assets: &LevelAssets,
    wanted: &[bool; MATERIALS],
) -> Result<VoxelMaterials, GpuError> {
    let scene = assets.scene();
    let mut out = VoxelMaterials {
        table: Vec::with_capacity(MATERIALS * MATERIAL_WORDS * 4),
        texels: Vec::new(),
        tables: Vec::new(),
    };
    let mut placed: Vec<Placed> = Vec::new();
    let mut rows = Vec::new();
    for (id, &wanted) in (0u16..).zip(wanted) {
        let texture = scene.texture_of(id);
        let found = placed.iter().find(|p| std::ptr::eq(p.texture, texture));
        let mut words = match found {
            Some(p) => p.words,
            None if wanted => {
                let words = place(&mut out.texels, texture)?;
                placed.push(Placed { texture, words });
                words
            }
            None => PLACEHOLDER,
        };
        let table = assets.spec_table(id);
        words[4] = spec_row(&mut rows, &mut out.tables, table);
        let visible = table.is_some_and(SpecTable::is_visible);
        let spots = u8::try_from(id).is_ok_and(has_spots);
        words[5] = u32::from(texture.is_filtered()) * SMOOTH
            + u32::from(visible) * SPEC_VISIBLE
            + u32::from(spots) * SPOTS;
        for word in words {
            push_u32(&mut out.table, word);
        }
    }
    Ok(out)
}
