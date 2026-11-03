//! The level's faces and movers: one vertex buffer for all of them, and each
//! frame an index list of those in view, grouped into one draw per material.
//! The level's own camera is the first view; each vista layer in the frame is
//! another, with its own frame and eye.

use std::ops::Range;

use mscent_render::clip::MAX_POLYGON_VERTICES;
use mscent_render::scene::{Scene, eye_of};
use mscent_render::{Camera, DynLight, LevelAssets, MoverPose, Vertex, VistaView};

use crate::device::{Gpu, GpuError};
use crate::dyn_lights::SetMasks;
use crate::face_data::{self, TABLE_LEN};
use crate::face_pipeline::{Compiled, VERTEX_BYTES, sampler};
use crate::face_views::{FaceView, POSE_BYTES, Shared, layer_view, push_pose};
use crate::finish::Finish;
use crate::lightmap::LightAtlas;
use crate::mover_casters::{MoverList, push_fan};
use crate::shadows::{self, ShadowMaps};
use crate::textures;

const INDEX_BYTES: usize = 4;
/// The most vista layers a level may put on the GPU: every slot full.
const MAX_LAYERS: usize =
    mscent_world::materials::VIEW_SLOTS as usize * mscent_world::MAX_VISTA_LAYERS as usize;

/// A run of the index list, with one material and one pose.
pub(crate) struct Draw {
    pub(crate) material: usize,
    pub(crate) pose: u32,
    pub(crate) indices: Range<u32>,
}

/// The pipelines of a cutout material's two draws, made only for a level that
/// has one.
struct Cutout {
    solid: wgpu::RenderPipeline,
    soft: wgpu::RenderPipeline,
}

pub(crate) struct FacePass {
    opaque: wgpu::RenderPipeline,
    cutout: Option<Cutout>,
    material_groups: Vec<wgpu::BindGroup>,
    /// By material group: whether it is a cutout.
    masked: Vec<bool>,
    /// By face: which of `material_groups` draws it.
    material_of: Vec<usize>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    lights: SetMasks,
    movers: MoverList,
    /// By face: its vertices, empty for one the rasterizer would refuse.
    corners: Vec<Range<u32>>,
    /// By material, this frame's visible faces, kept so a frame does not
    /// allocate them again.
    by_material: Vec<Vec<u32>>,
    listed: Vec<u8>,
    pose_bytes: Vec<u8>,
    /// The level's view first, then one for each vista layer of the level.
    views: Vec<FaceView>,
    pub(crate) atlas_size: (u32, u32),
}

/// The rasterizer refuses these, so they are not drawn here either.
fn drawable(poly: &[Vertex]) -> bool {
    let finite = poly.iter().all(|v| v.pos.is_finite());
    (3..=MAX_POLYGON_VERTICES).contains(&poly.len()) && finite
}

/// All faces' vertices, and which belong to whom.
fn vertex_bytes(scene: &Scene) -> Result<(Vec<u8>, Vec<Range<u32>>), GpuError> {
    let mut bytes = Vec::new();
    let mut corners = Vec::with_capacity(scene.polys().len());
    let mut next = 0u32;
    for (index, (poly, face)) in scene.polys().iter().zip(scene.faces()).enumerate() {
        if !drawable(poly) {
            corners.push(next..next);
            continue;
        }
        let texture = scene.texture_of(face.material);
        let (tw, th) = (texture.width() as f32, texture.height() as f32);
        let face_index = u32::try_from(index).map_err(|_| GpuError::TooLarge("faces"))?;
        for v in poly {
            for value in [v.pos.x, v.pos.y, v.pos.z, v.uv.x * tw, v.uv.y * th] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(&face_index.to_le_bytes());
        }
        let end = u32::try_from(poly.len())
            .ok()
            .and_then(|count| next.checked_add(count))
            .ok_or(GpuError::TooLarge("vertices"))?;
        corners.push(next..end);
        next = end;
    }
    Ok((bytes, corners))
}

/// The most index bytes a view can list: every face in view.
fn most_index_bytes(corners: &[Range<u32>]) -> Result<u64, GpuError> {
    corners
        .iter()
        .try_fold(0u64, |total, c| {
            let triangles = c.len().saturating_sub(2) as u64;
            total.checked_add(triangles.checked_mul(3 * INDEX_BYTES as u64)?)
        })
        .ok_or(GpuError::TooLarge("indices"))
}

pub(crate) fn buffer(
    gpu: &Gpu,
    label: &str,
    size: u64,
    usage: wgpu::BufferUsages,
) -> Result<wgpu::Buffer, GpuError> {
    let limits = gpu.device.limits();
    if size > limits.max_buffer_size {
        return Err(GpuError::TooLarge("a buffer"));
    }
    // A shader binds the whole of each storage buffer, as the poses are bound.
    let bound = usage.contains(wgpu::BufferUsages::STORAGE);
    if bound && size > limits.max_storage_buffer_binding_size {
        return Err(GpuError::TooLarge("a storage buffer"));
    }
    Ok(gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: usage | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }))
}

/// A storage buffer holding `bytes`, or `at_least` zero bytes where there are
/// none: a level can have no faces, and a binding must hold one element. More
/// than a shader may bind is an error, not a buffer it would read short.
pub(crate) fn storage(
    gpu: &Gpu,
    label: &str,
    bytes: &[u8],
    at_least: u64,
) -> Result<wgpu::Buffer, GpuError> {
    let size = (bytes.len() as u64).max(at_least);
    let buffer = buffer(gpu, label, size, wgpu::BufferUsages::STORAGE)?;
    gpu.queue.write_buffer(&buffer, 0, bytes);
    Ok(buffer)
}

/// The bind group of each material in use, whether each is a cutout, and by face
/// which it takes.
type Materials = (Vec<wgpu::BindGroup>, Vec<bool>, Vec<usize>);

/// The bind group of each material in use, whether it is a cutout, and which
/// each face takes. Unknown material ids all share the grey fallback, so a
/// hostile level cannot make a texture per id.
fn materials(
    gpu: &Gpu,
    layout: &wgpu::BindGroupLayout,
    scene: &Scene,
) -> Result<Materials, GpuError> {
    let mut seen: Vec<*const mscent_render::Texture> = Vec::new();
    let (mut groups, mut masked) = (Vec::new(), Vec::new());
    let mut material_of = Vec::with_capacity(scene.faces().len());
    for face in scene.faces() {
        let texture = scene.texture_of(face.material);
        let at = match seen.iter().position(|&t| std::ptr::eq(t, texture)) {
            Some(at) => at,
            None => {
                seen.push(texture);
                groups.push(textures::bind_group(gpu, layout, texture)?);
                masked.push(texture.is_masked());
                groups.len() - 1
            }
        };
        material_of.push(at);
    }
    Ok((groups, masked, material_of))
}

impl FacePass {
    pub(crate) fn new(
        gpu: &Gpu,
        assets: &LevelAssets,
        frame_uniform: &wgpu::Buffer,
        (finish, shadows): (Finish, &ShadowMaps),
    ) -> Result<Self, GpuError> {
        let scene = assets.scene();
        if assets.vista_layer_count() > MAX_LAYERS {
            return Err(GpuError::TooLarge("vista layers"));
        }
        // Before any face names its set, so that a set the flags cannot hold is refused first.
        let lights = SetMasks::new(gpu, scene.rooms().set_count())?;
        let atlas = LightAtlas::new(gpu, assets)?;
        let data = face_data::build(assets, &atlas);
        let pose_size = (assets.movers().len() as u64 + 1)
            .checked_mul(POSE_BYTES as u64)
            .ok_or(GpuError::TooLarge("movers"))?;
        let poses = buffer(gpu, "poses", pose_size, wgpu::BufferUsages::STORAGE)?;
        let (vertex_data, corners) = vertex_bytes(scene)?;
        let vertices = buffer(
            gpu,
            "faces",
            (vertex_data.len() as u64).max(VERTEX_BYTES as u64),
            wgpu::BufferUsages::VERTEX,
        )?;
        gpu.queue.write_buffer(&vertices, 0, &vertex_data);
        let index_bytes = most_index_bytes(&corners)?.max(INDEX_BYTES as u64);
        let indices = buffer(gpu, "faces", index_bytes, wgpu::BufferUsages::INDEX)?;
        let compiled =
            gpu.face_pipelines[finish.index()].get_or_init(|| Compiled::new(gpu, finish));
        let layouts = &compiled.layouts;
        let (material_groups, masked, material_of) = materials(gpu, &layouts.material, scene)?;
        let cutout = masked.contains(&true).then(|| {
            let [solid, soft] = compiled.cutout(gpu, finish).clone();
            Cutout { solid, soft }
        });
        // The atlas is read at its top level, so only the materials are filtered.
        let shared = Shared {
            layout: &layouts.frame,
            faces: storage(gpu, "face data", &data.faces, 160)?,
            levels: storage(gpu, "light levels", &data.levels, 16)?,
            tables: storage(gpu, "spec tables", &data.tables, 4 * TABLE_LEN as u64)?,
            atlas: &atlas.view,
            clamped: sampler(gpu, wgpu::AddressMode::ClampToEdge, Finish::Reference),
            repeat: sampler(gpu, wgpu::AddressMode::Repeat, finish),
            lights: &lights.buffer,
            shadows,
        };
        let mut views = vec![shared.view(gpu, frame_uniform, poses)];
        for _ in 0..assets.vista_layer_count() {
            views.push(layer_view(gpu, &shared)?);
        }
        let by_material = vec![Vec::new(); material_groups.len()];
        Ok(Self {
            opaque: compiled.opaque.clone(),
            cutout,
            material_groups,
            masked,
            material_of,
            vertices,
            indices,
            lights,
            movers: MoverList::new(gpu, assets, &corners)?,
            corners,
            by_material,
            listed: Vec::new(),
            pose_bytes: Vec::new(),
            views,
            atlas_size: atlas.size,
        })
    }

    /// Appends the triangles of `face` to the index list.
    fn list_face(&mut self, face: u32) {
        let Some(corners) = self.corners.get(face as usize) else {
            return;
        };
        // A convex polygon, as a fan from its first corner.
        push_fan(corners, &mut self.listed);
    }

    fn list_so_far(&self) -> u32 {
        (self.listed.len() / INDEX_BYTES) as u32
    }

    /// The faces in `visible`, one draw per material, into view `view`.
    fn list_world(&mut self, view: usize, visible: &[u32]) {
        for faces in &mut self.by_material {
            faces.clear();
        }
        for &face in visible {
            let material = self.material_of.get(face as usize).copied();
            if let Some(faces) = material.and_then(|m| self.by_material.get_mut(m)) {
                faces.push(face);
            }
        }
        let by_material = std::mem::take(&mut self.by_material);
        let mut draws = self
            .views
            .get_mut(view)
            .map(|v| std::mem::take(&mut v.draws));
        for (material, faces) in by_material.iter().enumerate() {
            let start = self.list_so_far();
            for &face in faces {
                self.list_face(face);
            }
            let indices = start..self.list_so_far();
            if let Some(draws) = draws.as_mut().filter(|_| !indices.is_empty()) {
                draws.push(Draw {
                    material,
                    pose: 0,
                    indices,
                });
            }
        }
        if let (Some(v), Some(draws)) = (self.views.get_mut(view), draws) {
            v.draws = draws;
        }
        self.by_material = by_material;
    }

    /// Every mover's faces that can be seen, each at its entry in `poses` or as
    /// authored: `list_movers` in `level/movers.rs`.
    fn list_movers(&mut self, assets: &LevelAssets, cam: &Camera, poses: &[MoverPose]) {
        let eye = eye_of(cam);
        let mut draws = Vec::new();
        for (i, mover) in assets.movers().iter().enumerate() {
            let pose = poses.get(i).copied().unwrap_or(MoverPose::AUTHORED);
            push_pose(&mut self.pose_bytes, pose, eye, [0.0; 2]);
            // View is along -Z, so all of it is behind the eye past this.
            if cam.world_to_camera(pose.to_world(mover.centre)).z > mover.radius {
                continue;
            }
            let authored_eye = pose.to_authored(eye).to_f64();
            for index in mover.faces.clone() {
                let Some(face) = assets.scene().faces().get(index) else {
                    continue;
                };
                if face.plane.signed_distance(authored_eye) <= 0.0 {
                    continue;
                }
                let start = self.list_so_far();
                self.list_face(index as u32);
                draws.push(Draw {
                    material: self.material_of.get(index).copied().unwrap_or(0),
                    pose: i as u32 + 1,
                    indices: start..self.list_so_far(),
                });
            }
        }
        if let Some(view) = self.views.first_mut() {
            view.draws = draws;
        }
    }

    /// Lists each vista layer's faces in a view of its own, with its frame from
    /// `frame_of` and its eye and pan in its one pose. Layers past those the
    /// level has, which cannot be, are left out.
    fn list_layers(
        &mut self,
        gpu: &Gpu,
        layers: &[VistaView],
        frame_of: impl Fn(&VistaView) -> Vec<u8>,
    ) {
        for (i, layer) in layers.iter().enumerate() {
            let Some(view) = self.views.get_mut(i + 1) else {
                return;
            };
            view.draws.clear();
            let mut pose = Vec::with_capacity(POSE_BYTES);
            push_pose(
                &mut pose,
                MoverPose::AUTHORED,
                eye_of(&layer.cam),
                layer.pan,
            );
            gpu.queue.write_buffer(&view.poses, 0, &pose);
            if let Some(uniform) = &view.uniform {
                gpu.queue.write_buffer(uniform, 0, &frame_of(layer));
            }
            self.list_world(i + 1, &layer.faces);
        }
    }

    /// The `Frame` of this frame's `i`th vista layer.
    pub(crate) fn layer_frame(&self, i: usize) -> Option<&wgpu::Buffer> {
        self.views.get(i + 1)?.uniform.as_ref()
    }

    /// Writes this frame's index list, on a larger buffer when the layers have
    /// made it longer than any before.
    fn write_indices(&mut self, gpu: &Gpu) -> Result<(), GpuError> {
        let needed = self.listed.len() as u64;
        if needed > self.indices.size() {
            let size = needed.checked_next_power_of_two().unwrap_or(needed);
            self.indices = buffer(gpu, "faces", size, wgpu::BufferUsages::INDEX)?;
        }
        gpu.queue.write_buffer(&self.indices, 0, &self.listed);
        Ok(())
    }

    /// Lists this frame's faces, the level's and then each of `layers`', and
    /// writes what the shader reads for them. `cam` sees `visible`.
    pub(crate) fn prepare(
        &mut self,
        gpu: &Gpu,
        assets: &LevelAssets,
        (cam, visible): (&Camera, &[u32]),
        (poses, lights): (&[MoverPose], &[DynLight]),
        layers: &[VistaView],
        frame_of: impl Fn(&VistaView) -> Vec<u8>,
    ) -> Result<(), GpuError> {
        self.listed.clear();
        self.pose_bytes.clear();
        push_pose(
            &mut self.pose_bytes,
            MoverPose::AUTHORED,
            eye_of(cam),
            [0.0; 2],
        );
        self.list_movers(assets, cam, poses);
        self.list_world(0, visible);
        self.list_layers(gpu, layers, frame_of);
        if let Some(view) = self.views.first() {
            gpu.queue.write_buffer(&view.poses, 0, &self.pose_bytes);
        }
        self.lights.write(gpu, lights);
        self.movers.list(assets, poses);
        self.write_indices(gpu)
    }

    /// The movers, for a light's shadow map.
    pub(crate) fn casters(&self) -> Option<shadows::MoverCasters<'_>> {
        Some(shadows::MoverCasters {
            vertices: &self.vertices,
            indices: &self.movers.indices,
            poses: &self.views.first()?.poses,
            draws: &self.movers.draws,
        })
    }

    fn bind(&self, pass: &mut wgpu::RenderPass, pipeline: &wgpu::RenderPipeline, view: &FaceView) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &view.bind_group, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
    }

    /// The draws of `view` whose material is a cutout or not, by `masked`.
    fn run(&self, pass: &mut wgpu::RenderPass, view: &FaceView, masked: bool) {
        for draw in &view.draws {
            let group = self.material_groups.get(draw.material);
            let is_masked = self.masked.get(draw.material).copied().unwrap_or(false);
            if let Some(group) = group.filter(|_| is_masked == masked) {
                pass.set_bind_group(1, group, &[]);
                pass.draw_indexed(draw.indices.clone(), 0, draw.pose..draw.pose + 1);
            }
        }
    }

    /// The soft texels of view `view`'s cutouts: before everything else of the
    /// faces, so that a face drawn behind one later covers it, as the software
    /// renderer's front-to-back order has it. View 0 is the level's own.
    pub(crate) fn draw_soft(&self, pass: &mut wgpu::RenderPass, view: usize) {
        let (Some(cutout), Some(view)) = (&self.cutout, self.views.get(view)) else {
            return;
        };
        self.bind(pass, &cutout.soft, view);
        self.run(pass, view, true);
    }

    /// The faces of view `view`, all but the soft texels of cutouts.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass, view: usize) {
        let Some(view) = self.views.get(view) else {
            return;
        };
        self.bind(pass, &self.opaque, view);
        self.run(pass, view, false);
        if let Some(cutout) = &self.cutout {
            self.bind(pass, &cutout.solid, view);
            self.run(pass, view, true);
        }
    }
}
