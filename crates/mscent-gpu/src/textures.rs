//! Material textures on the GPU: the albedo with the mips `mips.rs` made and a
//! cutout's coverage in its alpha, and the normal map with the specular mask in
//! its alpha.

use mscent_render::Texture;

use crate::device::Gpu;

/// `0xAARRGGBB` little-endian, the bytes a [`Texture`] holds.
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
/// A flat normal with a full mask, as [`mscent_render::texture::NormalView`] reads.
const FLAT_NORMAL_TEXEL: u32 = 0xff80_80ff;

fn create(gpu: &Gpu, width: u32, height: u32, levels: u32) -> wgpu::Texture {
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
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn write_level(gpu: &Gpu, texture: &wgpu::Texture, level: u32, w: u32, h: u32, texels: &[u8]) {
    let target = wgpu::TexelCopyTextureInfo {
        mip_level: level,
        ..texture.as_image_copy()
    };
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(w * 4),
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
    let texture = create(gpu, w, h, top + 1);
    for level in 0..=top {
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        let shift = level.min(31);
        let coverage = source.alpha_sampler(level);
        let bytes = bytes_of(lw, lh, |x, y| {
            let (x, y) = (x << shift, y << shift);
            let alpha = coverage.as_ref().map_or(u8::MAX, |c| c.nearest(x, y));
            source.texel_lod(x, y, level) & 0x00ff_ffff | u32::from(alpha) << 24
        });
        write_level(gpu, &texture, level, lw, lh, &bytes);
    }
    texture
}

fn normals(gpu: &Gpu, source: &Texture) -> wgpu::Texture {
    let (w, h) = if source.has_normals() {
        (source.width(), source.height())
    } else {
        (1, 1)
    };
    let texture = create(gpu, w, h, 1);
    let bytes = bytes_of(w, h, |x, y| {
        source.normal_texel(x, y).unwrap_or(FLAT_NORMAL_TEXEL)
    });
    write_level(gpu, &texture, 0, w, h, &bytes);
    texture
}

/// A material's textures, bound for one draw.
pub(crate) fn bind_group(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    source: &Texture,
) -> wgpu::BindGroup {
    let albedo = albedo(gpu, source).create_view(&Default::default());
    let normals = normals(gpu, source).create_view(&Default::default());
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
        ],
    })
}
