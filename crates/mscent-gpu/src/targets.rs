//! What a frame draws into, and getting it back to the CPU.

use mscent_render::{Camera, Framebuffer};

use crate::device::Gpu;
use crate::finish::Finish;
use crate::resolve::{DepthResolve, RESOLVED};

/// `0xAARRGGBB` little-endian, so a texel's bytes are a `Framebuffer` pixel's.
pub(crate) const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const TEXEL_BYTES: u32 = 4;

/// The colour and depth of a frame, and buffers to read them through.
pub(crate) struct Targets {
    size: (u32, u32),
    /// The finished colour, one sample a pixel, which is what is read.
    color: wgpu::Texture,
    color_view: wgpu::TextureView,
    /// Where a multisampled frame draws its colour before it is resolved.
    samples_view: Option<wgpu::TextureView>,
    depth: wgpu::Texture,
    pub(crate) depth_view: wgpu::TextureView,
    /// A multisampled frame's depth, one value a pixel, and what reads it.
    resolved_depth: Option<(wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
    color_read: wgpu::Buffer,
    depth_read: wgpu::Buffer,
    row_bytes: u32,
}

fn target(
    gpu: &Gpu,
    size: wgpu::Extent3d,
    format: wgpu::TextureFormat,
    samples: u32,
) -> wgpu::Texture {
    // A multisampled texture cannot be copied; its depth is read by a shader.
    // Particles read a frame's depth through a shader too.
    let read = if samples > 1 {
        wgpu::TextureUsages::TEXTURE_BINDING
    } else {
        wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING
    };
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | read,
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
    /// 8192, so none of this arithmetic can overflow. `resolve` is there when
    /// `finish` multisamples.
    pub(crate) fn new(
        gpu: &Gpu,
        (width, height): (u32, u32),
        finish: Finish,
        resolve: Option<&DepthResolve>,
    ) -> Self {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let samples = finish.samples();
        let color = target(gpu, size, COLOR, 1);
        let depth = target(gpu, size, DEPTH, samples);
        let depth_view = depth.create_view(&Default::default());
        let samples_view = (samples > 1)
            .then(|| target(gpu, size, COLOR, samples).create_view(&Default::default()));
        let resolved_depth = resolve.filter(|_| samples > 1).map(|resolve| {
            let texture = target(gpu, size, RESOLVED, 1);
            let view = texture.create_view(&Default::default());
            (texture, view, resolve.bind(gpu, &depth_view))
        });
        let row_bytes = (width * TEXEL_BYTES).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let bytes = u64::from(row_bytes) * u64::from(height);
        Self {
            size: (width, height),
            color_view: color.create_view(&Default::default()),
            samples_view,
            color,
            depth,
            depth_view,
            resolved_depth,
            color_read: read_buffer(gpu, bytes),
            depth_read: read_buffer(gpu, bytes),
            row_bytes,
        }
    }

    pub(crate) fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Where a frame draws its colour: into the samples, resolved into the
    /// finished colour at the end of the pass, or straight into it.
    pub(crate) fn color_attachment(&self) -> wgpu::RenderPassColorAttachment<'_> {
        let (view, resolve_target, store) = match &self.samples_view {
            Some(samples) => (samples, Some(&self.color_view), wgpu::StoreOp::Discard),
            None => (&self.color_view, None, wgpu::StoreOp::Store),
        };
        wgpu::RenderPassColorAttachment {
            view,
            depth_slice: None,
            resolve_target,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store,
            },
        }
    }

    /// Where particles draw, over the finished colour: they cover whole pixels,
    /// so they need no samples.
    pub(crate) fn finished_attachment(&self) -> wgpu::RenderPassColorAttachment<'_> {
        wgpu::RenderPassColorAttachment {
            view: &self.color_view,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        }
    }

    /// The frame's depth, one value a pixel, as a shader reads it: the depth
    /// itself, or a multisampled one resolved first with `resolve`.
    pub(crate) fn pixel_depth(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resolve: Option<&DepthResolve>,
    ) -> &wgpu::TextureView {
        match (&self.resolved_depth, resolve) {
            (Some((_, view, source)), Some(resolve)) => {
                resolve.run(encoder, source, view);
                view
            }
            _ => &self.depth_view,
        }
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

    /// Records the copies [`Self::read_into`] will read, resolving a
    /// multisampled depth first with `resolve`.
    pub(crate) fn copy_for_reading(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        with_depth: bool,
        resolve: Option<&DepthResolve>,
    ) {
        self.copy_out(encoder, self.color.as_image_copy(), &self.color_read);
        if !with_depth {
            return;
        }
        let depth = match (&self.resolved_depth, resolve) {
            (Some((texture, view, source)), Some(resolve)) => {
                resolve.run(encoder, source, view);
                texture.as_image_copy()
            }
            _ => wgpu::TexelCopyTextureInfo {
                aspect: wgpu::TextureAspect::DepthOnly,
                ..self.depth.as_image_copy()
            },
        };
        self.copy_out(encoder, depth, &self.depth_read);
    }

    /// Copies the finished frame into `fb`, which must be this size, leaving its
    /// depth as it was unless `with_depth`. `None` when the device has gone or the
    /// buffers cannot be read.
    pub(crate) fn read_into(
        &self,
        gpu: &Gpu,
        cam: &Camera,
        fb: &mut Framebuffer,
        with_depth: bool,
    ) -> Option<()> {
        map(gpu, &self.color_read)?;
        if with_depth {
            map(gpu, &self.depth_read)?;
        }
        let (row, width) = (self.row_bytes as usize, self.size.0 as usize);
        let (pixels, depths) = fb.planes_mut();
        {
            let color = self.color_read.get_mapped_range(..).ok()?;
            for (texels, out) in rows(&color, row, pixels, width) {
                for (pixel, texel) in out.iter_mut().zip(texels) {
                    *pixel = u32::from_le_bytes(*texel);
                }
            }
        }
        self.color_read.unmap();
        if with_depth {
            {
                let depth = self.depth_read.get_mapped_range(..).ok()?;
                for (texels, out) in rows(&depth, row, depths, width) {
                    for (stored, texel) in out.iter_mut().zip(texels) {
                        *stored = cam.inv_z_of_gpu_depth(f32::from_le_bytes(*texel));
                    }
                }
            }
            self.depth_read.unmap();
        }
        Some(())
    }
}
