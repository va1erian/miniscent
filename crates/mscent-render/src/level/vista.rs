//! What a viewport face shows: a stack of layers, each a sealed room of the same
//! level seen from a camera that follows the player's.
//!
//! The main scene draws first, so a vista shades only the pixels that show it.
//! A layer's eye is `pos + (player - centre) * parallax`: at 0 the view turns but
//! never shifts, like a distant sky; at 1/16 a room reads as 16 times its size.
//! See `docs/viewports.md`.

use std::f64::consts::TAU;

use mscent_math::{Quat, Vec3};
use mscent_world::lightmap::{LightFace, LuxelDensity, PointLight};
use mscent_world::materials::{VIEW_SLOTS, is_self_lit};
use mscent_world::vista_scale::luxel_factor;
use mscent_world::{Bsp, Entity, Face, Islands, LeafId, Level, Portals};

use super::{FAR_VISTA, LevelRenderer, NEAR, Quality, View};
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{DrawStats, PolyList, Vertex};
use crate::scene::eye_of;
use crate::shade::Fog;
use crate::voxel::fill::{FillContext, fill_voxels};
use crate::voxel::list::VoxelList;

/// One layer of a vista, as the renderer keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    pos: Vec3,
    centre: Vec3,
    slot: u8,
    parallax: f32,
    /// Axis times radians per second, turning the layer's camera.
    spin: Vec3,
    /// Texels per second, on self-lit faces.
    pan: [f32; 2],
    island: u32,
    fog: Option<Fog>,
}

/// A level's vista layers, far to near within each slot.
#[derive(Clone, Debug)]
pub(super) struct Vistas {
    layers: Vec<Layer>,
    islands: Islands,
    /// Islands that hold a layer and are not the player start's, sorted, each with
    /// how much coarser than the play space its faces are baked.
    remote: Vec<(u32, f32)>,
}

/// One layer's polygons for a frame. No camera: its slot is not seen.
#[derive(Default)]
pub(super) struct LayerList {
    faces: PolyList,
    cam: Option<Camera>,
    /// Room for a panned face's shifted vertices, kept so a frame makes none.
    scratch: Vec<Vertex>,
    /// The voxel chunks of the room, listed from the layer's own eye.
    voxels: VoxelList,
}

impl LayerList {
    pub(super) fn voxel_triangles(&self) -> u32 {
        self.voxels.triangles
    }
}

/// Triangles one layer's voxels may submit, and all layers' together in a frame.
/// The main scene has [`MAX_VOXEL_TRIS`] to itself, since each list is its own.
/// A layer is far and small on screen, and its miniature picks coarse detail by
/// distance in its own room units, so a range seldom comes near the cap.
const LAYER_VOXEL_TRIS: u32 = 12_000;
const VISTA_VOXEL_TRIS: u32 = 24_000;

/// A time to animate by: seconds of the game's own clock, never negative or
/// not a number.
fn clean_time(seconds: f32) -> f32 {
    if seconds.is_finite() && seconds > 0.0 {
        seconds
    } else {
        0.0
    }
}

/// The layer's camera turn at `time`: about the axis of `spin` by its length
/// times `time`. Whole turns are dropped so a long session keeps its precision.
/// No spin gives `None`, so the player's own orientation passes through bit for bit.
fn spin_rotation(spin: Vec3, time: f32) -> Option<Quat> {
    let rate = f64::from(spin.length());
    if rate == 0.0 || !rate.is_finite() {
        return None;
    }
    let angle = (rate * f64::from(time)).rem_euclid(TAU);
    Some(Quat::from_axis_angle(spin, angle as f32))
}

/// Texels a layer's self-lit faces have scrolled at `time`, or `None` when it
/// does not pan (so nothing is copied).
fn pan_texels(pan: [f32; 2], time: f32) -> Option<[f32; 2]> {
    let [u, v] = pan.map(|rate| rate * time);
    (u != 0.0 || v != 0.0).then_some([u, v])
}

/// The island the first player start is in, if the level has one in empty space.
fn player_island(level: &Level, bsp: &Bsp, islands: &Islands) -> Option<u32> {
    islands.island_of(bsp.leaf_at_f32(level.player_start()?))
}

/// Fog at this range or further means a layer asked for none, and pays for none.
fn layer_fog(fog_color: u32, start: f32, end: f32) -> Option<Fog> {
    if end >= mscent_world::MAX_VISTA_FOG {
        return None;
    }
    Fog::new(0xff00_0000 | fog_color, start, end)
}

impl Vistas {
    /// The layers of `level`. One whose eye is in solid has no room to show.
    pub(super) fn new(level: &Level, bsp: &Bsp, portals: &Portals) -> Self {
        let islands = Islands::build(bsp, portals);
        let mut found: Vec<(u8, Layer)> = Vec::new();
        for entity in level.vista_layers() {
            let Entity::VistaLayer {
                pos,
                centre,
                slot,
                layer,
                parallax,
                spin,
                pan,
                fog_start,
                fog_end,
                fog_color,
                ..
            } = *entity
            else {
                continue;
            };
            if let Some(island) = islands.island_of(bsp.leaf_at_f32(pos)) {
                let fog = layer_fog(fog_color, fog_start, fog_end);
                found.push((
                    layer,
                    Layer {
                        pos,
                        centre,
                        slot,
                        parallax,
                        spin,
                        pan,
                        island,
                        fog,
                    },
                ));
            }
        }
        found.sort_by_key(|&(order, layer)| (layer.slot, order));
        let home = player_island(level, bsp, &islands);
        let away = found.iter().filter(|(_, l)| Some(l.island) != home);
        let mut remote: Vec<(u32, f32)> = Vec::new();
        for (_, l) in away {
            // Layers sharing a room share its luxels: the coarsest factor of them.
            let factor = luxel_factor(l.parallax);
            match remote.iter_mut().find(|(island, _)| *island == l.island) {
                Some(entry) => entry.1 = entry.1.max(factor),
                None => remote.push((l.island, factor)),
            }
        }
        remote.sort_unstable_by_key(|&(island, _)| island);
        Self {
            layers: found.into_iter().map(|(_, layer)| layer).collect(),
            islands,
            remote,
        }
    }

    /// Whether a point is in a layer's room that the player does not start in.
    fn holds(&self, bsp: &Bsp, pos: Vec3) -> bool {
        self.in_remote(bsp.leaf_at_f32(pos))
    }

    /// The lamps that light the play space. The bake keeps them all, since solid
    /// stops a far lamp's rays; meshes and voxels light without shadows, so a lamp
    /// in a layer's room must not reach them.
    pub(super) fn play_lights(&self, bsp: &Bsp, lights: &[PointLight]) -> Vec<PointLight> {
        let kept = lights.iter().filter(|l| !self.holds(bsp, l.position));
        kept.copied().collect()
    }

    /// The lamps that light a surface made without a bake at `pos`: all of them in
    /// a layer's room, whose surfaces are baked with its own lamps, else the play
    /// space's. Cheap when no layer has a room of its own.
    pub(super) fn lamps_at<'a>(
        &self,
        bsp: &Bsp,
        pos: Vec3,
        all: &'a [PointLight],
        play: &'a [PointLight],
    ) -> &'a [PointLight] {
        if !self.remote.is_empty() && self.holds(bsp, pos) {
            all
        } else {
            play
        }
    }

    /// Sets how finely each face is baked: sky materials not at all, faces in a
    /// layer's room coarsely. A mover face has no leaves, so it stays as it was.
    pub(super) fn set_densities(&self, faces: &[Face], light_faces: &mut [LightFace]) {
        for (face, light) in faces.iter().zip(light_faces) {
            let coarse = (face.front_leaves.iter()).filter_map(|&leaf| self.factor_at(leaf));
            let coarse = coarse.fold(None, |m: Option<f32>, f| Some(m.map_or(f, |m| m.max(f))));
            light.density = match (is_self_lit(face.material), coarse) {
                (true, _) => LuxelDensity::None,
                (false, Some(factor)) => LuxelDensity::Coarse(factor),
                (false, None) => LuxelDensity::Full,
            };
        }
    }

    /// How much coarser than the play space `leaf` is baked, if it is in a layer's room.
    fn factor_at(&self, leaf: LeafId) -> Option<f32> {
        let island = self.islands.island_of(leaf)?;
        let at = self
            .remote
            .binary_search_by_key(&island, |&(i, _)| i)
            .ok()?;
        self.remote.get(at).map(|&(_, factor)| factor)
    }

    fn in_remote(&self, leaf: LeafId) -> bool {
        self.factor_at(leaf).is_some()
    }
}

impl LevelRenderer {
    /// Lists every layer whose slot shows in `holes`, through the part of the
    /// screen those holes cover, as it looks `time` seconds in. Returns how many
    /// faces were listed.
    pub(super) fn list_vistas(
        &self,
        view: &View,
        (width, height): (u32, u32),
        holes: &PolyList,
        time: f32,
        out: &mut Vec<LayerList>,
    ) -> u32 {
        let time = clean_time(time);
        let mut listed = 0;
        let mut left = VISTA_VOXEL_TRIS;
        let layers = &self.vistas.layers;
        out.resize_with(layers.len(), LayerList::default);
        for (layer, list) in layers.iter().zip(out.iter_mut()) {
            list.cam = None;
            list.voxels.chunks.clear();
            list.voxels.triangles = 0;
            let Some(window) = holes.bounds_of(u32::from(layer.slot)) else {
                continue;
            };
            let eye = View {
                pos: layer.pos + (view.pos - layer.centre) * layer.parallax,
                orientation: match spin_rotation(layer.spin, time) {
                    Some(turn) => turn * view.orientation,
                    None => view.orientation,
                },
            };
            let Some(cam) = eye.camera(self.cfg.fov_y, NEAR, FAR_VISTA, width, height) else {
                continue;
            };
            let islands = &self.vistas.islands;
            let pan = pan_texels(layer.pan, time).map(|texels| (texels, &mut list.scratch));
            let out = (&mut list.faces, &mut list.voxels.candidates);
            let st = (self.scene).list_layer(&cam, islands, layer.island, window, out, pan);
            listed += st.drawn.len() as u32;
            list.voxels.budget = Some(left.min(LAYER_VOXEL_TRIS));
            let voxels = self.scene.voxels();
            list.voxels.prepare(voxels, &cam, eye_of(&cam), |_| 0);
            left = left.saturating_sub(list.voxels.triangles);
            list.cam = Some(cam);
        }
        listed
    }

    /// Marks the holes in the rows `fb` holds, then draws each slot that shows.
    pub(super) fn fill_vistas(
        &self,
        fb: &mut Framebuffer,
        holes: &PolyList,
        lists: &[LayerList],
        quality: Quality,
    ) -> DrawStats {
        let mut total = DrawStats::default();
        let seen = self.scene.fill_holes(fb, holes);
        for slot in (0..VIEW_SLOTS).filter(|s| seen >> s & 1 != 0) {
            fb.open_view(u32::from(slot), self.cfg.sky_top, self.cfg.sky_bottom);
            let of_slot = self.vistas.layers.iter().zip(lists);
            let of_slot = of_slot.filter(|(layer, _)| layer.slot == slot);
            for (i, (layer, list)) in of_slot.enumerate() {
                if i > 0 {
                    fb.next_view_layer();
                }
                let st = self.fill_layer(fb, layer, list, quality);
                total.pixels += st.pixels;
                total.spans += st.spans;
            }
            fb.close_view();
        }
        total
    }

    /// Shaded at the frame's quality, by the layer's own eye: a far room gets the
    /// bump and highlights a near one does. Only the player's dynamic lights are
    /// left out, since they cannot reach a sealed room. Voxels first, as in the
    /// main scene.
    fn fill_layer(
        &self,
        fb: &mut Framebuffer,
        layer: &Layer,
        list: &LayerList,
        quality: Quality,
    ) -> DrawStats {
        let Some(cam) = list.cam.as_ref() else {
            return DrawStats::default();
        };
        let eye = eye_of(cam);
        let texture_of = |id: u16| self.scene.texture_of(id);
        let ctx = FillContext {
            scene: self.scene.voxels(),
            cam,
            eye,
            fog: layer.fog,
            bump: quality != Quality::Low,
            specular: quality == Quality::High,
            lights: &[],
            spec: &self.spec,
            texture_of: &texture_of,
        };
        let mut total = fill_voxels(fb, &list.voxels, &ctx);
        let faces = (self.scene).fill_frame(fb, &list.faces, None, &mut |i, face| {
            self.face_shader(i, face, eye, quality, &[], layer.fog)
        });
        total.pixels += faces.pixels;
        total.spans += faces.spans;
        total
    }
}
