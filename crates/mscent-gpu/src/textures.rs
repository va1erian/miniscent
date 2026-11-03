//! Material textures on the GPU: the albedo with the mips `mips.rs` made and a
//! cutout's coverage in its alpha, and the normal map with the specular mask in
//! its alpha, and the height map for parallax.

use mscent_render::Texture;

use crate::device::{Gpu, GpuError};

/// `0xAARRGGBB` little-endian, the bytes a [`Texture`] holds.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// A flat normal with a full mask, as [`mscent_render::texture::NormalView`] reads.
/// One byte a texel: the height, 255 the face's plane.
const HEIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R8Unorm;
const FLAT_NORMAL_TEXEL: u32 = 0xff80_80ff;

fn create(gpu: &Gpu, size: (u32, u32), levels: u32, format: wgpu::TextureFormat) -> wgpu::Texture {
    let (width, height) = size;
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("material"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_level(gpu: &Gpu, texture: &wgpu::Texture, at: (u32, u32, u32), texels: &[u8]) {
    write_rows(gpu, texture, at, texels, 4);
}

/// `texels`, `bytes` to a texel, as the `w` by `h` level `level` of `texture`.
fn write_rows(
    gpu: &Gpu,
    texture: &wgpu::Texture,
    (level, w, h): (u32, u32, u32),
    texels: &[u8],
    bytes: u32,
) {
    let target = wgpu::TexelCopyTextureInfo {
        mip_level: level,
        ..texture.as_image_copy()
    };
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(w * bytes),
        rows_per_image: None,
    };
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    gpu.queue.write_texture(target, texels, layout, size);
}

/// The texels of a `w` by `h` image, as bytes.
fn bytes_of(w: u32, h: u32, at: impl Fn(i32, i32) -> u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            bytes.extend_from_slice(&at(x, y).to_le_bytes());
        }
    }
    bytes
}

fn albedo(gpu: &Gpu, source: &Texture) -> wgpu::Texture {
    let (w, h) = (source.width(), source.height());
    let top = source.mip_count();
    let texture = create(gpu, (w, h), top + 1, FORMAT);
    for level in 0..=top {
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        let shift = level.min(31);
        let coverage = source.alpha_sampler(level);
        let bytes = bytes_of(lw, lh, |x, y| {
            let (x, y) = (x << shift, y << shift);
            let alpha = coverage.as_ref().map_or(u8::MAX, |c| c.nearest(x, y));
            source.texel_lod(x, y, level) & 0x00ff_ffff | u32::from(alpha) << 24
        });
        write_level(gpu, &texture, (level, lw, lh), &bytes);
    }
    texture
}

fn normals(gpu: &Gpu, source: &Texture) -> wgpu::Texture {
    let (w, h) = if source.has_normals() {
        (source.width(), source.height())
    } else {
        (1, 1)
    };
    let texture = create(gpu, (w, h), 1, FORMAT);
    let bytes = bytes_of(w, h, |x, y| {
        source.normal_texel(x, y).unwrap_or(FLAT_NORMAL_TEXEL)
    });
    write_level(gpu, &texture, (0, w, h), &bytes);
    texture
}

/// The height map of `source`, lifted so its highest texel is 255 (the march
/// starts at the face's plane, which the relief's top is), with the albedo's chain
/// of levels. A material with none, or none to march, gets one dark texel.
fn heights(gpu: &Gpu, source: &Texture) -> wgpu::Texture {
    let plane = source
        .height_plane()
        .filter(|p| p.depth() > 0 && p.top() > 0);
    let (w, h) = (source.width(), source.height());
    let Some(plane) = plane else {
        let texture = create(gpu, (1, 1), 1, HEIGHT_FORMAT);
        write_rows(gpu, &texture, (0, 1, 1), &[0], 1);
        return texture;
    };
    let top = source.mip_count();
    let texture = create(gpu, (w, h), top + 1, HEIGHT_FORMAT);
    let lift = u8::MAX - plane.top();
    for level in 0..=top {
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        let lifted: Vec<u8> = plane
            .level(level)
            .iter()
            .map(|&t| t.saturating_add(lift))
            .collect();
        write_rows(gpu, &texture, (level, lw, lh), &lifted, 1);
    }
    texture
}

/// A material's textures, bound for one draw. One wider or taller than this GPU
/// holds is an error: a texture file can be up to `png::MAX_DIMENSION` a side.
pub(crate) fn bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    source: &Texture,
) -> Result<wgpu::BindGroup, GpuError> {
    let side = gpu.device.limits().max_texture_dimension_2d;
    let longest = source.width().max(source.height());
    if longest > side {
        return Err(GpuError::TooLarge("a material's texture"));
    }
    // A texture has one level for each halving of its longest side, and no more.
    if source.mip_count() >= u32::BITS - longest.leading_zeros() {
        return Err(GpuError::TooLarge("a material's mip levels"));
    }
    let albedo = albedo(gpu, source).create_view(&Default::default());
    let normals = normals(gpu, source).create_view(&Default::default());
    let heights = heights(gpu, source).create_view(&Default::default());
    Ok(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("material"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&albedo),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&normals),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&heights),
            },
        ],
    }))
}
