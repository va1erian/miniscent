//! A frame shown in a window straight from the GPU, with nothing read back: the
//! scene, the white flash and the 2D layer the game draws its HUD, feedback and
//! menus into, composited and scaled into the window's letterbox
//! (`shaders/present.wgsl`).

use crate::device::{Gpu, GpuError};

/// `Show` of `present.wgsl`.
const UNIFORM_BYTES: u64 = 48;
/// The layer's texels, `0xAARRGGBB` little-endian as a `Framebuffer` holds them.
const LAYER: wgpu::TextureFormat = wgpu::TextureFormat::Bgra8Unorm;

/// What goes over a frame and where it lands in the window.
pub struct Overlay<'a> {
    /// The 2D layer, its colour premultiplied by its alpha as `draw2d` leaves a
    /// layer cleared to nothing, and its size: the frame's, or smaller, for 2D
    /// drawn at a lower resolution than the world and shown by whole pixels.
    pub layer: &'a [u32],
    pub layer_size: (u32, u32),
    /// The white flash over the scene and under the layer, `0..=256`.
    pub white: u32,
    /// The window's inner size, and where the frame lands in it: `x, y, width,
    /// height`, as `mscent_platform::scale::fit_rect` places it.
    pub window: (u32, u32),
    pub rect: (u32, u32, u32, u32),
}

/// A window's surface and what composites a frame into it.
pub struct Presenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    configured: bool,
    compositor: Compositor,
}

/// What composites a frame into a texture of one format.
pub(crate) struct Compositor {
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// The layer's texture, made again when the frame's size changes.
    layer: Option<(wgpu::Texture, wgpu::TextureView)>,
}

fn pipeline(gpu: &Gpu, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
    let module = gpu
        .device
        .create_shader_module(wgpu::include_wgsl!("../shaders/present.wgsl"));
    let constants = [("SRGB", f64::from(u8::from(format.is_srgb())))];
    let options = wgpu::PipelineCompilationOptions {
        constants: &constants,
        ..Default::default()
    };
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("present"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: None,
                compilation_options: options.clone(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: None,
                compilation_options: options,
                targets: &[Some(format.into())],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

impl Presenter {
    /// A surface on `window` for `gpu`. The frame is written as it is, so a
    /// format that stores what it is given is taken where the window offers one.
    pub fn new(
        gpu: &Gpu,
        window: impl Into<wgpu::SurfaceTarget<'static>>,
    ) -> Result<Self, GpuError> {
        let surface = (gpu.instance)
            .create_surface(window)
            .map_err(|e| GpuError::NoSurface(e.to_string()))?;
        let caps = surface.get_capabilities(&gpu.wgpu_adapter);
        let format = caps.formats.iter().find(|f| !f.is_srgb());
        let format = *format
            .or(caps.formats.first())
            .ok_or_else(|| GpuError::NoSurface("no format this GPU can draw".into()))?;
        let mailbox = caps.present_modes.contains(&wgpu::PresentMode::Mailbox);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width: 1,
            height: 1,
            // A frame shown as soon as it is drawn: input latency.
            desired_maximum_frame_latency: 1,
            present_mode: if mailbox {
                wgpu::PresentMode::Mailbox
            } else {
                wgpu::PresentMode::Fifo
            },
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            view_formats: vec![],
        };
        Ok(Self {
            compositor: Compositor::new(gpu, format),
            surface,
            config,
            configured: false,
        })
    }

    /// The surface at the window's size; false for a window with no area.
    fn configure(&mut self, gpu: &Gpu, (width, height): (u32, u32)) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        let side = gpu.device.limits().max_texture_dimension_2d;
        let size = (width.min(side), height.min(side));
        if !self.configured || (self.config.width, self.config.height) != size {
            (self.config.width, self.config.height) = size;
            self.surface.configure(&gpu.device, &self.config);
            self.configured = true;
        }
        true
    }

    /// The window's next texture: `None` for a frame to skip, as one with the
    /// window hidden or a swap that timed out.
    fn next(&mut self) -> Result<Option<wgpu::SurfaceTexture>, GpuError> {
        use wgpu::CurrentSurfaceTexture as Next;
        match self.surface.get_current_texture() {
            Next::Success(texture) => Ok(Some(texture)),
            Next::Suboptimal(texture) => {
                self.configured = false;
                Ok(Some(texture))
            }
            Next::Timeout | Next::Occluded => Ok(None),
            Next::Outdated => {
                self.configured = false;
                Ok(None)
            }
            Next::Lost => Err(GpuError::NoSurface("the window's surface was lost".into())),
            Next::Validation => Err(GpuError::Rejected("the window's next frame".into())),
        }
    }

    /// Records the frame in `scene`, of the size `overlay.layer` is, into the
    /// window's next texture, which the caller presents once it is submitted.
    /// `None` when there is nothing to show this frame.
    pub(crate) fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        scene: (&wgpu::TextureView, (u32, u32)),
        overlay: &Overlay,
    ) -> Result<Option<wgpu::SurfaceTexture>, GpuError> {
        if !self.configure(gpu, overlay.window) {
            return Ok(None);
        }
        let Some(texture) = self.next()? else {
            return Ok(None);
        };
        let view = texture.texture.create_view(&Default::default());
        self.compositor
            .encode(gpu, encoder, scene, overlay, &view)?;
        Ok(Some(texture))
    }
}

impl Compositor {
    pub(crate) fn new(gpu: &Gpu, format: wgpu::TextureFormat) -> Self {
        let uniform = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("present"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            pipeline: pipeline(gpu, format),
            uniform,
            layer: None,
        }
    }

    /// The layer's texture for a frame of `size`, holding `pixels`.
    fn layer(
        &mut self,
        gpu: &Gpu,
        size: (u32, u32),
        pixels: &[u32],
    ) -> Result<&wgpu::TextureView, GpuError> {
        let area = (size.0 as usize).checked_mul(size.1 as usize);
        if area != Some(pixels.len()) {
            return Err(GpuError::Rejected("a layer not the frame's size".into()));
        }
        let extent = wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        };
        if self.layer.as_ref().map(|(t, _)| t.size()) != Some(extent) {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("2D layer"),
                size: extent,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: LAYER,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            self.layer = Some((texture, view));
        }
        let (texture, view) = self
            .layer
            .as_ref()
            .ok_or(GpuError::Rejected("layer".into()))?;
        let bytes: Vec<u8> = pixels.iter().flat_map(|p| p.to_le_bytes()).collect();
        let layout = wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.0 * 4),
            rows_per_image: None,
        };
        gpu.queue
            .write_texture(texture.as_image_copy(), &bytes, layout, extent);
        Ok(view)
    }

    /// Records the frame in `scene`, of the size `overlay.layer` is, composited
    /// into `target`, which is `overlay.window` in size.
    pub(crate) fn encode(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        (scene, size): (&wgpu::TextureView, (u32, u32)),
        overlay: &Overlay,
        target: &wgpu::TextureView,
    ) -> Result<(), GpuError> {
        let layer = self.layer(gpu, overlay.layer_size, overlay.layer)?.clone();
        let (x, y, w, h) = overlay.rect;
        let whole = w > 0 && w % size.0 == 0 && h % size.1 == 0 && w / size.0 == h / size.1;
        let floats = [
            x as f32,
            y as f32,
            w as f32,
            h as f32,
            size.0 as f32,
            size.1 as f32,
        ];
        let mut bytes: Vec<u8> = floats.iter().flat_map(|v| v.to_le_bytes()).collect();
        bytes.extend_from_slice(&(overlay.white.min(256) as f32).to_le_bytes());
        bytes.extend_from_slice(&u32::from(whole).to_le_bytes());
        let (lw, lh) = overlay.layer_size;
        for value in [lw as f32, lh as f32, 0.0, 0.0] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        gpu.queue.write_buffer(&self.uniform, 0, &bytes);
        let entries = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: self.uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(scene),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&layer),
            },
        ];
        let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("present"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.draw(0..3, 0..1);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "present_tests.rs"]
mod present_tests;
