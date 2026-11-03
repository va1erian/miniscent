//! The part of a voxel frame that does not depend on the rows being drawn: which
//! chunks, at which level of detail, with their vertices projected once so that
//! bands share them read-only. Nothing here allocates once its lists have grown.

use mscent_math::{Aabb, Vec3};

use super::VoxelScene;
use super::flood::Flood;
use crate::camera::{Camera, Frustum};

/// Triangles a frame submits before the governor steps in.
pub const MAX_VOXEL_TRIS: u32 = 25_000;
/// Distances to half and to quarter detail, in chunk sizes.
const LOD_DISTANCE: [f32; 2] = [3.0, 6.0];
/// How much farther than its switching distance a chunk must come back from,
/// so a chunk near the line does not flip every frame.
const LOD_HYSTERESIS: f32 = 0.9;
/// Bounds of the governor's scale on the distances.
const GOVERNOR_MIN: f32 = 0.125;
/// Steps of the scale after a frame over budget, and one well under it.
const GOVERNOR_DOWN: f32 = 0.8;
const GOVERNOR_UP: f32 = 1.05;
/// Share of the budget below which the distances may grow back.
const GOVERNOR_ROOM: f32 = 0.7;
/// Largest screen coordinate kept, in 1/16 pixels. A vertex just past the near
/// plane can project this far; far past the screen the error cannot be seen.
const MAX_SCREEN: f32 = (1 << 27) as f32;
/// Sub-pixel steps per pixel.
pub(super) const SUBPIXEL: f32 = 16.0;

/// A projected vertex, snapped so two triangles sharing it agree exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct ScreenV {
    pub x: i32,
    pub y: i32,
    /// `1/depth`, or zero for a vertex in front of the near plane.
    pub iz: f32,
}

/// One chunk of a frame.
#[derive(Clone, Copy, Debug)]
pub struct ListedChunk {
    pub chunk: u32,
    /// Its level of detail: 0 is full.
    pub lod: u8,
    /// Where its vertices start in the frame's list.
    pub(crate) start: usize,
    /// Screen rows `first..last` it may cover.
    pub(crate) first: i32,
    pub(crate) last: i32,
    /// Bit `i`: dynamic light `i` can reach it.
    pub lights: u8,
}

/// A frame's voxel chunks, and what is kept from one frame to the next: which
/// chunks, at which detail, is the same choice for every renderer.
#[derive(Default)]
pub struct VoxelList {
    pub(crate) chunks: Vec<ListedChunk>,
    pub(crate) screen: Vec<ScreenV>,
    /// Filled by the BSP walk: chunks of visible leaves.
    pub candidates: Vec<u32>,
    /// Those that can be seen, with how far off they are.
    near: Vec<(f32, u32)>,
    /// By chunk: the frame it was last taken, so one in many leaves counts once.
    taken: Vec<u32>,
    frame: u32,
    /// By chunk: its level of detail last frame.
    lods: Vec<u8>,
    /// The governor's scale on the distances; zero until first used.
    scale: f32,
    /// Triangles this list may submit before it drops to the lowest detail;
    /// `None` is the frame's own [`MAX_VOXEL_TRIS`].
    pub budget: Option<u32>,
    flood: Flood,
    flooded: Option<u32>,
    pub triangles: u32,
    /// For a renderer that projects on its own: no screen positions or rows.
    unprojected: bool,
}

impl VoxelList {
    /// A list for a renderer that projects vertices itself, as a GPU does.
    pub fn unprojected() -> Self {
        Self {
            unprojected: true,
            ..Self::default()
        }
    }

    /// This frame's chunks, nearest first.
    pub fn chunks(&self) -> &[ListedChunk] {
        &self.chunks
    }

    /// Replaces the list from [`Self::candidates`]. `lights` says which dynamic
    /// lights reach a leaf, as a bit per light.
    pub fn prepare(
        &mut self,
        scene: &VoxelScene,
        cam: &Camera,
        eye: Vec3,
        lights: impl Fn(mscent_world::LeafId) -> u8,
    ) {
        self.chunks.clear();
        self.screen.clear();
        self.triangles = 0;
        let count = scene.world.chunks.len();
        self.taken.resize(count, 0);
        self.lods.resize(count, 0);
        self.frame = self.frame.wrapping_add(1).max(1);
        if self.scale == 0.0 {
            self.scale = 1.0;
        }
        let frustum = cam.frustum();
        let candidates = std::mem::take(&mut self.candidates);
        // Chunks of one volume come in runs far more often than not, but a run
        // can be broken by leaves of another: each volume floods once at most,
        // and a change of volume repeats it, which costs only time.
        self.flooded = None;
        for &c in &candidates {
            self.consider(scene, cam, &frustum, eye, c);
        }
        self.candidates = candidates;
        // A cave is one BSP leaf, so the walk's order says nothing within it. Near
        // chunks first: the z-buffer then saves the shading of what they hide, and
        // an overloaded frame loses detail from the far ones.
        let mut near = std::mem::take(&mut self.near);
        near.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, c) in &near {
            self.take(scene, cam, eye, c, &lights);
        }
        near.clear();
        self.near = near;
        self.govern();
    }

    fn cap(&self) -> u32 {
        self.budget.unwrap_or(MAX_VOXEL_TRIS)
    }

    /// Notes chunk `c` for this frame if it can be seen at all.
    fn consider(&mut self, scene: &VoxelScene, cam: &Camera, frustum: &Frustum, eye: Vec3, c: u32) {
        let i = c as usize;
        let Some(chunk) = scene.world.chunks.get(i) else {
            return;
        };
        if self.taken.get(i) == Some(&self.frame) {
            return;
        }
        self.taken[i] = self.frame;
        let bounds = &chunk.lods[0].bounds;
        if self.flood_reaches(scene, eye, c) && in_view(cam, frustum, bounds) {
            self.near.push((distance_to_box(bounds, eye), c));
        }
    }

    fn take(
        &mut self,
        scene: &VoxelScene,
        cam: &Camera,
        eye: Vec3,
        c: u32,
        lights: &impl Fn(mscent_world::LeafId) -> u8,
    ) {
        let i = c as usize;
        let Some(chunk) = scene.world.chunks.get(i) else {
            return;
        };
        let lod = self.pick_lod(scene, eye, i);
        let mesh = &chunk.lods[usize::from(lod)];
        let start = self.screen.len();
        let (first, last) = if self.unprojected {
            (0, 0)
        } else {
            self.screen
                .extend(mesh.vertices.iter().map(|v| project(cam, v.pos)));
            row_span(&self.screen[start..], cam.height() as i32)
        };
        let reach = chunk.leaves.iter().fold(0u8, |m, &l| m | lights(l));
        self.triangles = self.triangles.saturating_add(mesh.triangles.len() as u32);
        self.chunks.push(ListedChunk {
            chunk: c,
            lod,
            start,
            first,
            last,
            lights: reach,
        });
    }

    fn flood_reaches(&mut self, scene: &VoxelScene, eye: Vec3, c: u32) -> bool {
        let Some(&(v, index)) = scene.place.get(c as usize) else {
            return true;
        };
        let Some(volume) = scene.world.volumes.get(v as usize) else {
            return true;
        };
        if self.flooded != Some(v) {
            self.flood.run(volume, eye);
            self.flooded = Some(v);
        }
        self.flood.reached(index as usize)
    }

    /// Level of detail by distance. Once a frame has submitted its budget, the
    /// rest (the farthest, as chunks come nearest first) take the lowest level,
    /// so an overloaded view loses distant detail first.
    fn pick_lod(&mut self, scene: &VoxelScene, eye: Vec3, i: usize) -> u8 {
        let Some(chunk) = scene.world.chunks.get(i) else {
            return 2;
        };
        if self.triangles >= self.cap() {
            self.lods[i] = 2;
            return 2;
        }
        let bounds = &chunk.lods[0].bounds;
        let size = scene
            .place
            .get(i)
            .and_then(|&(v, _)| scene.world.volumes.get(v as usize))
            .map_or(4.0, |v| v.chunk_size);
        let d = distance_to_box(bounds, eye) / (size * self.scale);
        let last = self.lods[i];
        let at = |level: u8| {
            // A chunk at this level or finer switches at the full distance,
            // and one already coarser comes back a little nearer.
            let line = LOD_DISTANCE[usize::from(level)];
            if last > level {
                line * LOD_HYSTERESIS
            } else {
                line
            }
        };
        let lod = if d > at(1) {
            2
        } else if d > at(0) {
            1
        } else {
            0
        };
        self.lods[i] = lod;
        lod
    }

    /// Pulls the distances in after a frame over budget and lets them back out
    /// slowly, with a gap between the two so they do not oscillate.
    fn govern(&mut self) {
        let t = self.triangles as f32;
        let budget = self.cap() as f32;
        if t > budget {
            self.scale = (self.scale * GOVERNOR_DOWN).max(GOVERNOR_MIN);
        } else if t < budget * GOVERNOR_ROOM {
            self.scale = (self.scale * GOVERNOR_UP).min(1.0);
        }
    }
}

fn distance_to_box(b: &Aabb, p: Vec3) -> f32 {
    let gap = |v: f32, lo: f32, hi: f32| (lo - v).max(v - hi).max(0.0);
    Vec3::new(
        gap(p.x, b.min.x, b.max.x),
        gap(p.y, b.min.y, b.max.y),
        gap(p.z, b.min.z, b.max.z),
    )
    .length()
}

/// False only when all eight corners lie outside one frustum plane.
pub(crate) fn in_view(cam: &Camera, frustum: &Frustum, b: &Aabb) -> bool {
    let corners: [Vec3; 8] = std::array::from_fn(|i| {
        let pick = |bit: usize, lo: f32, hi: f32| if i & bit == 0 { lo } else { hi };
        cam.world_to_camera(Vec3::new(
            pick(1, b.min.x, b.max.x),
            pick(2, b.min.y, b.max.y),
            pick(4, b.min.z, b.max.z),
        ))
    });
    (0..6).all(|plane| corners.iter().any(|&c| frustum.distance(plane, c) >= 0.0))
}

/// Screen position in sub-pixels, kept in the range the fill's arithmetic takes.
pub(super) fn snap(v: f32) -> i32 {
    (v * SUBPIXEL).round().clamp(-MAX_SCREEN, MAX_SCREEN) as i32
}

fn project(cam: &Camera, pos: Vec3) -> ScreenV {
    let c = cam.world_to_camera(pos);
    if -c.z < cam.near() {
        return ScreenV::default();
    }
    match cam.project(c) {
        Some(s) => ScreenV {
            x: snap(s.x),
            y: snap(s.y),
            iz: s.inv_z,
        },
        None => ScreenV::default(),
    }
}

/// Rows a chunk may cover. A vertex past the near plane could land anywhere.
fn row_span(verts: &[ScreenV], height: i32) -> (i32, i32) {
    let mut lo = i32::MAX;
    let mut hi = i32::MIN;
    for v in verts {
        if v.iz <= 0.0 {
            return (0, height);
        }
        lo = lo.min(v.y);
        hi = hi.max(v.y);
    }
    let row = SUBPIXEL as i32;
    (lo.div_euclid(row), hi.div_euclid(row).saturating_add(2))
}
