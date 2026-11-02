//! What a frame draws into, and getting it back to the CPU.

use mscent_render::{Camera, Framebuffer};

use crate::device::Gpu;

/// `0xAARRGGBB` little-endian, so a texel's bytes are a `Framebuffer` pixel's.
pub(crate) const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const TEXEL_BYTES: u32 = 4;

/// The colour and depth of a frame, and buffers to read them through.
pub(crate) struct Targets {
    size: (u32, u32),
    color: wgpu::Texture,
    depth: wgpu::Texture,
    pub(crate) color_view: wgpu::TextureView,
    pub(crate) depth_view: wgpu::TextureView,
    color_read: wgpu::Buffer,
    depth_read: wgpu::Buffer,
    row_bytes: u32,
}

fn target(gpu: &Gpu, size: wgpu::Extent3d, format: wgpu::TextureFormat) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

fn read_buffer(gpu: &Gpu, bytes: u64) -> wgpu::Buffer {
    gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: bytes,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    })
}

/// Waits for `buffer` to map. `None` when the device has gone.
fn map(gpu: &Gpu, buffer: &wgpu::Buffer) -> Option<()> {
    let (done, finished) = std::sync::mpsc::channel();
    buffer.map_async(wgpu::MapMode::Read, .., move |result| {
        let _ = done.send(result);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
    finished.try_recv().ok()?.ok()
}

/// Each row of a read buffer with its texels, past the padding wgpu asks for,
/// beside its row of `out`. A loop over one row at a time is what the compiler
/// can turn into a straight copy, at a frame's eight million pixels.
fn rows<'a, T>(
    bytes: &'a [u8],
    row_bytes: usize,
    out: &'a mut [T],
    width: usize,
) -> impl Iterator<Item = (&'a [[u8; 4]], &'a mut [T])> {
    let texels = bytes
        .chunks_exact(row_bytes)
        .map(move |row| row[..width * TEXEL_BYTES as usize].as_chunks::<4>().0);
    texels.zip(out.chunks_exact_mut(width))
}

impl Targets {
    /// `width` and `height` come from a [`Framebuffer`], which holds them to
    /// 8192, so none of this arithmetic can overflow.
    pub(crate) fn new(gpu: &Gpu, width: u32, height: u32) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let color = target(gpu, size, COLOR);
        let depth = target(gpu, size, DEPTH);
        let row_bytes = (width * TEXEL_BYTES).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let bytes = u64::from(row_bytes) * u64::from(height);
        Self {
            size: (width, height),
            color_view: color.create_view(&Default::default()),
            depth_view: depth.create_view(&Default::default()),
            color,
            depth,
            color_read: read_buffer(gpu, bytes),
            depth_read: read_buffer(gpu, bytes),
            row_bytes,
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    fn copy_out(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        from: wgpu::TexelCopyTextureInfo,
        to: &wgpu::Buffer,
    ) {
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(self.row_bytes),
            rows_per_image: None,
        };
        let buffer = wgpu::TexelCopyBufferInfo { buffer: to, layout };
        encoder.copy_texture_to_buffer(from, buffer, self.color.size());
    }

    /// Records the copies [`Self::read_into`] will read.
    pub(crate) fn copy_for_reading(&self, encoder: &mut wgpu::CommandEncoder) {
        self.copy_out(encoder, self.color.as_image_copy(), &self.color_read);
        let depth = wgpu::TexelCopyTextureInfo {
            aspect: wgpu::TextureAspect::DepthOnly,
            ..self.depth.as_image_copy()
        };
        self.copy_out(encoder, depth, &self.depth_read);
    }

    /// Copies the finished frame into `fb`, which must be this size. `None`
    /// when the device has gone or the buffers cannot be read.
    pub(crate) fn read_into(&self, gpu: &Gpu, cam: &Camera, fb: &mut Framebuffer) -> Option<()> {
        map(gpu, &self.color_read)?;
        map(gpu, &self.depth_read)?;
        let (row, width) = (self.row_bytes as usize, self.size.0 as usize);
        let (pixels, depths) = fb.planes_mut();
        {
            let color = self.color_read.get_mapped_range(..).ok()?;
            for (texels, out) in rows(&color, row, pixels, width) {
                for (pixel, texel) in out.iter_mut().zip(texels) {
                    *pixel = u32::from_le_bytes(*texel);
                }
            }
            let depth = self.depth_read.get_mapped_range(..).ok()?;
            for (texels, out) in rows(&depth, row, depths, width) {
                for (stored, texel) in out.iter_mut().zip(texels) {
                    *stored = cam.inv_z_of_gpu_depth(f32::from_le_bytes(*texel));
                }
            }
        }
        self.color_read.unmap();
        self.depth_read.unmap();
        Some(())
    }
}
