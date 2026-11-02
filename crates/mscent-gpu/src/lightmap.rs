//! The lightmap on the GPU: every face's colour levels and baked directions,
//! packed into one texture. A fetch stays inside its face's rectangle, so
//! there is no bleeding and no need for gutters.

use mscent_render::LevelAssets;

use crate::device::{Gpu, GpuError};

/// `Rgba16Float` is the filterable format that holds the bake's range and sign.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const TEXEL_BYTES: usize = 8;
/// Wide enough for any face the baker makes, and a width every GPU can hold.
const ATLAS_WIDTH: u32 = 4096;

/// Where a face's levels and directions are, in luxels of the atlas.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    /// `x, y, max x, max y`, as the shader clamps them.
    pub(crate) fn as_vec4(self) -> [f32; 4] {
        [
            self.x as f32,
            self.y as f32,
            self.w.saturating_sub(1) as f32,
            self.h.saturating_sub(1) as f32,
        ]
    }
}

/// What the atlas holds of one face.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct FaceRects {
    pub levels: Vec<Rect>,
    pub dir: Rect,
}

pub(crate) struct LightAtlas {
    pub view: wgpu::TextureView,
    pub size: (u32, u32),
    /// By face; empty for one with no light in the lightmap.
    pub faces: Vec<FaceRects>,
}

/// Rows of rectangles, tallest first so a row wastes little. Returns where each
/// went, by its place in `sizes`, and the height used.
fn pack(sizes: &[(u32, u32)], width: u32) -> Option<(Vec<Rect>, u32)> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(sizes[i].1));
    let mut rects = vec![Rect::default(); sizes.len()];
    let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
    for i in order {
        let (w, h) = sizes[i];
        if w > width {
            return None;
        }
        if x + w > width {
            y = y.checked_add(row)?;
            (x, row) = (0, 0);
        }
        rects[i] = Rect { x, y, w, h };
        x += w;
        row = row.max(h);
    }
    Some((rects, y.checked_add(row)?))
}

/// The IEEE half nearest `value`. Values past its range saturate, and those too
/// small for it go to zero, far below one level of colour.
fn half_of(value: f32) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    if value.is_nan() {
        return sign;
    }
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if exponent >= 31 {
        return sign | 0x7bff;
    }
    if exponent <= 0 {
        return sign;
    }
    let mantissa = (bits >> 13) & 0x3ff;
    let round_up = (bits >> 12) & 1;
    let magnitude = ((exponent as u32) << 10 | mantissa) + round_up;
    sign | magnitude.min(0x7bff) as u16
}

/// A level of a face, or its directions, as they go into the texture.
fn write_rect(
    texels: &mut [u8],
    width: u32,
    rect: Rect,
    at: impl Fn(u32, u32) -> [f32; 3],
) -> Option<()> {
    for y in 0..rect.h {
        for x in 0..rect.w {
            let start = ((rect.y + y) as usize * width as usize + (rect.x + x) as usize)
                .checked_mul(TEXEL_BYTES)?;
            let out = texels.get_mut(start..start + TEXEL_BYTES)?;
            let [r, g, b] = at(x, y);
            for (slot, value) in out.as_chunks_mut::<2>().0.iter_mut().zip([r, g, b, 1.0]) {
                *slot = half_of(value).to_le_bytes();
            }
        }
    }
    Some(())
}

impl LightAtlas {
    pub(crate) fn new(gpu: &Gpu, assets: &LevelAssets) -> Result<Self, GpuError> {
        let too_large = || GpuError::TooLarge("the lightmap");
        let count = assets.lightmap().projections().len();
        let mut sizes = Vec::new();
        for face in 0..count {
            for level in assets.light_mips().levels(face) {
                sizes.push((level.width(), level.height()));
            }
            if let Some(grid) = assets.lightmap().face_grid(face) {
                sizes.push((grid.width(), grid.height()));
            }
        }
        let limit = gpu.device.limits().max_texture_dimension_2d;
        let width = ATLAS_WIDTH.min(limit);
        let (rects, height) = pack(&sizes, width).ok_or_else(too_large)?;
        let height = height.max(1);
        if height > limit {
            return Err(too_large());
        }
        let bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(TEXEL_BYTES))
            .ok_or_else(too_large)?;
        let mut texels = vec![0u8; bytes];
        let mut placed = rects.into_iter();
        let mut faces = Vec::with_capacity(count);
        for face in 0..count {
            let mut rects = FaceRects::default();
            for level in assets.light_mips().levels(face) {
                let rect = placed.next().ok_or_else(too_large)?;
                write_rect(&mut texels, width, rect, |x, y| {
                    let [r, g, b, _] = level.cell(x, y)[0];
                    [r, g, b]
                })
                .ok_or_else(too_large)?;
                rects.levels.push(rect);
            }
            if let Some(grid) = assets.lightmap().face_grid(face) {
                rects.dir = placed.next().ok_or_else(too_large)?;
                write_rect(&mut texels, width, rects.dir, |x, y| {
                    let d = grid.at_clamped(x, y).dir;
                    [d.x, d.y, d.z]
                })
                .ok_or_else(too_large)?;
            }
            faces.push(rects);
        }
        let view = upload(gpu, width, height, &texels);
        Ok(Self {
            view,
            size: (width, height),
            faces,
        })
    }
}

fn upload(gpu: &Gpu, width: u32, height: u32, texels: &[u8]) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("lightmap"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(width * TEXEL_BYTES as u32),
        rows_per_image: None,
    };
    gpu.queue
        .write_texture(texture.as_image_copy(), texels, layout, size);
    texture.create_view(&Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_keeps_what_the_bake_holds() {
        assert_eq!(half_of(0.0), 0);
        assert_eq!(half_of(1.0), 0x3c00);
        assert_eq!(half_of(-1.0), 0xbc00);
        assert_eq!(half_of(0.5), 0x3800);
        assert_eq!(half_of(2.0), 0x4000);
    }

    #[test]
    fn half_saturates_and_never_makes_an_infinity_or_a_nan() {
        assert_eq!(half_of(1.0e9), 0x7bff);
        assert_eq!(half_of(f32::INFINITY), 0x7bff);
        assert_eq!(half_of(f32::NAN), 0);
        assert_eq!(half_of(1.0e-9), 0);
    }

    #[test]
    fn rectangles_do_not_overlap_and_stay_in_the_width() {
        let sizes = [(100, 7), (4096, 3), (50, 50), (4000, 2), (97, 50), (1, 1)];
        let (rects, height) = pack(&sizes, 4096).expect("fits");
        for (i, a) in rects.iter().enumerate() {
            assert_eq!((a.w, a.h), sizes[i]);
            assert!(a.x + a.w <= 4096 && a.y + a.h <= height);
            for b in &rects[i + 1..] {
                let apart =
                    a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y;
                assert!(apart, "{a:?} and {b:?}");
            }
        }
    }

    #[test]
    fn a_rectangle_wider_than_the_atlas_is_refused() {
        assert_eq!(pack(&[(4097, 1)], 4096), None);
    }
}
