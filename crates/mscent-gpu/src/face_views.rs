//! The views the faces are drawn in: the level's own camera, and each vista
//! layer's, each with the `Frame` and poses its bind group gives the shader.

use mscent_math::Vec3;
use mscent_render::MoverPose;

use crate::device::{Gpu, GpuError};
use crate::faces::{Draw, buffer};
use crate::shadows::ShadowMaps;

/// A `Pose` of the shader: a matrix, the eye and the pan.
pub(crate) const POSE_BYTES: usize = 96;

/// One camera's draws: the frame and poses its shader reads, and what it lists.
pub(crate) struct FaceView {
    pub(crate) bind_group: wgpu::BindGroup,
    /// A layer's own `Frame`; the level's view shares the one every pass reads.
    pub(crate) uniform: Option<wgpu::Buffer>,
    pub(crate) poses: wgpu::Buffer,
    pub(crate) draws: Vec<Draw>,
}

/// A pose's matrix, eye and pan, as the shader reads them.
pub(crate) fn push_pose(out: &mut Vec<u8>, pose: MoverPose, eye: Vec3, pan: [f32; 2]) {
    let origin = pose.to_world(Vec3::ZERO);
    let axis = |v: Vec3| pose.to_world(v) - origin;
    let (x, y, z) = (
        axis(Vec3::new(1.0, 0.0, 0.0)),
        axis(Vec3::new(0.0, 1.0, 0.0)),
        axis(Vec3::new(0.0, 0.0, 1.0)),
    );
    let authored = pose.to_authored(eye);
    let values = [
        x.x, x.y, x.z, 0.0, y.x, y.y, y.z, 0.0, z.x, z.y, z.z, 0.0, origin.x, origin.y, origin.z,
        1.0, authored.x, authored.y, authored.z, 0.0, pan[0], pan[1], 0.0, 0.0,
    ];
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
}

/// What every view's bind group shares, beside its own frame and poses.
pub(crate) struct Shared<'a> {
    pub(crate) layout: &'a wgpu::BindGroupLayout,
    pub(crate) faces: wgpu::Buffer,
    pub(crate) levels: wgpu::Buffer,
    pub(crate) tables: wgpu::Buffer,
    pub(crate) atlas: &'a wgpu::TextureView,
    pub(crate) clamped: wgpu::Sampler,
    pub(crate) repeat: wgpu::Sampler,
    pub(crate) lights: &'a wgpu::Buffer,
    pub(crate) shadows: &'a ShadowMaps,
}

impl Shared<'_> {
    pub(crate) fn view(&self, gpu: &Gpu, frame: &wgpu::Buffer, poses: wgpu::Buffer) -> FaceView {
        let resources = [
            frame.as_entire_binding(),
            self.faces.as_entire_binding(),
            self.levels.as_entire_binding(),
            poses.as_entire_binding(),
            self.tables.as_entire_binding(),
            wgpu::BindingResource::TextureView(self.atlas),
            wgpu::BindingResource::Sampler(&self.clamped),
            wgpu::BindingResource::Sampler(&self.repeat),
            self.lights.as_entire_binding(),
            wgpu::BindingResource::TextureView(&self.shadows.cube),
            wgpu::BindingResource::Sampler(&self.shadows.sampler),
        ];
        let entries: Vec<wgpu::BindGroupEntry> = (0u32..)
            .zip(resources)
            .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
            .collect();
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("faces"),
            layout: self.layout,
            entries: &entries,
        });
        FaceView {
            bind_group,
            uniform: None,
            poses,
            draws: Vec::new(),
        }
    }
}

/// A vista layer's view, with a `Frame` of its own.
pub(crate) fn layer_view(gpu: &Gpu, shared: &Shared) -> Result<FaceView, GpuError> {
    let frame = buffer(
        gpu,
        "layer frame",
        crate::FRAME_BYTES,
        wgpu::BufferUsages::UNIFORM,
    )?;
    let poses = buffer(
        gpu,
        "layer pose",
        POSE_BYTES as u64,
        wgpu::BufferUsages::STORAGE,
    )?;
    let mut view = shared.view(gpu, &frame, poses);
    view.uniform = Some(frame);
    Ok(view)
}
