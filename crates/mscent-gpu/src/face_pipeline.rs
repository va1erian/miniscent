//! The face shader's pipelines and samplers. A cutout material draws twice, as
//! `raster_masked.rs` decides per texel: its solid texels like any face, and its
//! soft ones blended over what is there with depth left alone.

use std::sync::OnceLock;

use mscent_math::epsilon::MIN_LENGTH_F32;

use crate::dyn_lights::SET_SHIFT;
use mscent_render::bump::{MAX_GAIN, MIN_FLAT, MIN_GAIN, RELIEF};
use mscent_render::light_sampler::MIN_BLEND;
use mscent_render::lighting_shader::{BUMP_FULL, BUMP_GONE};
use mscent_render::raster::{LONG_AXIS_LIMIT, SHARP_RATE};
use mscent_render::relief::{MAX_STEPS, MIN_STEPS, PARALLAX_FULL, PARALLAX_GONE};
use mscent_render::texel_light::{KNEE, MIN_SPEC, ROLL, SPEC_GAIN};

use crate::common::with_common;
use crate::device::Gpu;
use crate::face_data::TABLE_LEN;
use crate::finish::Finish;
use crate::targets::{COLOR, DEPTH};

/// Position, texel coordinates, face.
pub(crate) const VERTEX_BYTES: usize = 24;
const SHADER: &str = concat!(
    include_str!("../shaders/common.wgsl"),
    include_str!("../shaders/shading.wgsl"),
    include_str!("../shaders/shadow_sample.wgsl"),
    include_str!("../shaders/face.wgsl"),
);

/// Which texels of a material a pipeline draws.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Texels {
    /// All of them, for a material with no cutout.
    Opaque,
    /// Those of full coverage, which write depth.
    Solid,
    /// Those partly covered, blended in without depth.
    Soft,
}

impl Texels {
    /// The shader's `MASK`.
    fn mask(self) -> f64 {
        match self {
            Self::Opaque => 0.0,
            Self::Solid => 1.0,
            Self::Soft => 2.0,
        }
    }

    /// A soft texel lays its colour over what is there by its coverage, and
    /// keeps the target's alpha.
    fn blend(self) -> Option<wgpu::BlendState> {
        (self == Self::Soft).then_some(wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::SrcAlpha,
                dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::Zero,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        })
    }
}

/// The bind groups every face pipeline takes, spelled out so that one set of
/// them serves all three pipelines: `@group(0)` and `@group(1)` of `face.wgsl`.
pub(crate) struct Layouts {
    pub(crate) frame: wgpu::BindGroupLayout,
    pub(crate) material: wgpu::BindGroupLayout,
    pipeline: wgpu::PipelineLayout,
}

fn entry(binding: u32, ty: wgpu::BindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty,
        count: None,
    }
}

fn read_only() -> wgpu::BindingType {
    wgpu::BindingType::Buffer {
        ty: wgpu::BufferBindingType::Storage { read_only: true },
        has_dynamic_offset: false,
        min_binding_size: None,
    }
}

fn texture() -> wgpu::BindingType {
    wgpu::BindingType::Texture {
        sample_type: wgpu::TextureSampleType::Float { filterable: true },
        view_dimension: wgpu::TextureViewDimension::D2,
        multisampled: false,
    }
}

impl Layouts {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let device = &gpu.device;
        let uniform = wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        };
        let filtering = wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering);
        let frame = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("faces"),
            entries: &[
                entry(0, uniform),
                entry(1, read_only()),
                entry(2, read_only()),
                entry(3, read_only()),
                entry(4, read_only()),
                entry(5, texture()),
                entry(6, filtering),
                entry(7, filtering),
                entry(8, read_only()),
                entry(
                    9,
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::CubeArray,
                        multisampled: false,
                    },
                ),
                entry(
                    10,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                ),
            ],
        });
        let material = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                entry(0, texture()),
                entry(1, texture()),
                entry(2, texture()),
            ],
        });
        let pipeline = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("faces"),
            bind_group_layouts: &[Some(&frame), Some(&material)],
            immediate_size: 0,
        });
        Self {
            frame,
            material,
            pipeline,
        }
    }
}

fn pipeline(gpu: &Gpu, layouts: &Layouts, finish: Finish, texels: Texels) -> wgpu::RenderPipeline {
    let device = &gpu.device;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("faces"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let constants = [
        ("SHARP_RATE", f64::from(SHARP_RATE)),
        ("LONG_AXIS_LIMIT", f64::from(LONG_AXIS_LIMIT)),
        ("MIN_BLEND", f64::from(MIN_BLEND)),
        ("PARALLAX_FULL", f64::from(PARALLAX_FULL)),
        ("PARALLAX_GONE", f64::from(PARALLAX_GONE)),
        ("PARALLAX_MIN_STEPS", f64::from(MIN_STEPS)),
        ("PARALLAX_MAX_STEPS", f64::from(MAX_STEPS)),
        ("BUMP_FULL", f64::from(BUMP_FULL)),
        ("BUMP_GONE", f64::from(BUMP_GONE)),
        ("MIN_FLAT", f64::from(MIN_FLAT)),
        ("MIN_GAIN", f64::from(MIN_GAIN)),
        ("MAX_GAIN", f64::from(MAX_GAIN)),
        ("RELIEF", f64::from(RELIEF)),
        ("KNEE", f64::from(KNEE)),
        ("ROLL", f64::from(ROLL)),
        ("SPEC_GAIN", f64::from(SPEC_GAIN)),
        ("MIN_SPEC", f64::from(MIN_SPEC)),
        ("MIN_LENGTH", f64::from(MIN_LENGTH_F32)),
        ("SET_SHIFT", f64::from(SET_SHIFT)),
        ("TABLE_LEN", TABLE_LEN as f64),
        ("FINE", f64::from(u8::from(finish == Finish::Fine))),
        ("MASK", texels.mask()),
    ];
    let constants: Vec<_> = constants
        .into_iter()
        .chain(crate::shadows::sample_constants())
        .collect();
    let options = wgpu::PipelineCompilationOptions {
        constants: &with_common(&constants),
        ..Default::default()
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("faces"),
        layout: Some(&layouts.pipeline),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: None,
            compilation_options: options.clone(),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: VERTEX_BYTES as u64,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &wgpu::vertex_attr_array![
                    0 => Float32x3, 1 => Float32x2, 2 => Uint32
                ],
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: None,
            compilation_options: options,
            targets: &[Some(wgpu::ColorTargetState {
                format: COLOR,
                blend: texels.blend(),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        // The walk has already dropped faces that point away.
        primitive: Default::default(),
        // Depth is linear in `1/z` as in software: larger is nearer, 0.0 is nothing.
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH,
            depth_write_enabled: Some(texels != Texels::Soft),
            depth_compare: Some(wgpu::CompareFunction::Greater),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: finish.multisample(),
        multiview_mask: None,
        cache: None,
    })
}

/// A finish's face pipelines, compiled once for a device, as the editor makes
/// a scene on every edit.
pub(crate) struct Compiled {
    pub(crate) layouts: Layouts,
    pub(crate) opaque: wgpu::RenderPipeline,
    /// The solid and soft draws of a cutout, compiled with the first level that
    /// has one.
    cutout: OnceLock<[wgpu::RenderPipeline; 2]>,
}

impl Compiled {
    pub(crate) fn new(gpu: &Gpu, finish: Finish) -> Self {
        let layouts = Layouts::new(gpu);
        Self {
            opaque: pipeline(gpu, &layouts, finish, Texels::Opaque),
            layouts,
            cutout: OnceLock::new(),
        }
    }

    pub(crate) fn cutout(&self, gpu: &Gpu, finish: Finish) -> &[wgpu::RenderPipeline; 2] {
        self.cutout.get_or_init(|| {
            [Texels::Solid, Texels::Soft].map(|t| pipeline(gpu, &self.layouts, finish, t))
        })
    }
}

pub(crate) fn sampler(gpu: &Gpu, address: wgpu::AddressMode, finish: Finish) -> wgpu::Sampler {
    // Software takes one whole level per span; a fine frame blends two, which
    // anisotropic filtering needs.
    let mipmap_filter = match finish {
        Finish::Reference => wgpu::MipmapFilterMode::Nearest,
        Finish::Fine => wgpu::MipmapFilterMode::Linear,
    };
    gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: address,
        address_mode_v: address,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter,
        anisotropy_clamp: finish.anisotropy(),
        ..Default::default()
    })
}
