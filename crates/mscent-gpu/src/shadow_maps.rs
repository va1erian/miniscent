//! The cube maps shadows are drawn into, and whether this device can hold them
//! (`shadows.rs`).

use crate::device::{Gpu, GpuError};
use crate::shadows::{LAYERS, MAP, SIDE};
use crate::targets::DEPTH;

/// Whether the faces' shader can take a cube array at all: the adapter holds
/// them, and a device has the layers of one cube. Without that the faces are
/// built without any shadow lookup (`shadow_none.wgsl`) and draw none.
pub(crate) fn cube_arrays(gpu: &Gpu) -> bool {
    let flags = gpu.wgpu_adapter.get_downlevel_capabilities().flags;
    flags.contains(wgpu::DownlevelFlags::CUBE_ARRAY_TEXTURES)
        && gpu.device.limits().max_texture_array_layers >= 6
}

/// Whether the maps themselves fit this device: sides and layers, and a format
/// that can be drawn into and read.
fn fits(gpu: &Gpu) -> bool {
    let limits = gpu.device.limits();
    let wanted = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
    let usable = gpu.wgpu_adapter.get_texture_format_features(MAP);
    cube_arrays(gpu)
        && limits.max_texture_dimension_2d >= SIDE
        && limits.max_texture_array_layers >= LAYERS
        && usable.allowed_usages.contains(wanted)
}

/// A drawable map texture, or the error wgpu gave for it.
fn drawn_maps(
    gpu: &Gpu,
) -> Result<(wgpu::TextureView, Vec<wgpu::TextureView>, wgpu::TextureView), GpuError> {
    gpu.checked(|| {
        let make = |format, layers, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("shadow maps"),
                size: wgpu::Extent3d {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let drawn = wgpu::TextureUsages::RENDER_ATTACHMENT;
        let maps = make(MAP, LAYERS, drawn | wgpu::TextureUsages::TEXTURE_BINDING);
        let layers = (0..LAYERS)
            .map(|layer| {
                maps.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: layer,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let cube = cube_view(&maps);
        let depth = make(DEPTH, 1, drawn).create_view(&Default::default());
        Ok((cube, layers, depth))
    })
}

fn cube_view(maps: &wgpu::Texture) -> wgpu::TextureView {
    maps.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::CubeArray),
        ..Default::default()
    })
}

/// One cube of a texel, which the shader never reads: what the faces are bound to
/// when the maps do not fit.
fn placeholder(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("no shadow maps"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 6,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: MAP,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    cube_view(&texture)
}

/// What is drawn into, once the maps fit.
pub(crate) struct Drawable {
    /// A view of each layer, to draw into.
    pub(crate) layers: Vec<wgpu::TextureView>,
    pub(crate) depth: wgpu::TextureView,
}

/// The cube maps, one array of them, and what the faces' shader reads them with.
pub(crate) struct ShadowMaps {
    /// The cube array and its sampler, bound at 9 and 10 of the faces' group; none
    /// where the device has no cube arrays, which the faces are then built without.
    pub(crate) bound: Option<(wgpu::TextureView, wgpu::Sampler)>,
    /// Present where the maps fit this device and were made: a device that cannot
    /// hold them, or ran out of memory making them, draws no shadows.
    pub(crate) drawable: Option<Drawable>,
}

impl ShadowMaps {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        if !cube_arrays(gpu) {
            return Self {
                bound: None,
                drawable: None,
            };
        }
        // Taps are single texels: the filter is the shader's own.
        let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let made = if fits(gpu) {
            drawn_maps(gpu).ok()
        } else {
            None
        };
        match made {
            Some((cube, layers, depth)) => Self {
                bound: Some((cube, sampler)),
                drawable: Some(Drawable { layers, depth }),
            },
            None => Self {
                bound: Some((placeholder(gpu), sampler)),
                drawable: None,
            },
        }
    }

    /// Whether lights can hold maps.
    pub(crate) fn ready(&self) -> bool {
        self.drawable.is_some()
    }
}
