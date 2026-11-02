//! What a viewport face shows: a stack of layers, each a sealed room of the same
//! level seen from a camera that follows the player's.
//!
//! The main scene draws first, so a vista shades only the pixels that show it.
//! A layer's eye is `pos + (player - centre) * parallax`: at 0 the view turns but
//! never shifts, like a distant sky; at 1/16 a room reads as 16 times its size.
//! See `docs/viewports.md`.

use mscent_math::Vec3;
use mscent_world::materials::VIEW_SLOTS;
use mscent_world::{Bsp, Entity, Islands, Level, Portals};

use super::{FAR_VISTA, LevelRenderer, NEAR, Quality, View};
use crate::camera::Camera;
use crate::framebuffer::Framebuffer;
use crate::raster::{DrawStats, PolyList};
use crate::scene::eye_of;
use crate::shade::Fog;

/// One layer of a vista, as the renderer keeps it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layer {
    pos: Vec3,
    centre: Vec3,
    slot: u8,
    parallax: f32,
    island: u32,
    fog: Option<Fog>,
}

/// A level's vista layers, far to near within each slot.
#[derive(Clone, Debug)]
pub(super) struct Vistas {
    layers: Vec<Layer>,
    islands: Islands,
}

/// One layer's polygons for a frame. No camera: its slot is not seen.
#[derive(Clone, Debug, Default)]
pub(super) struct LayerList {
    faces: PolyList,
    cam: Option<Camera>,
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
                        island,
                        fog,
                    },
                ));
            }
        }
        found.sort_by_key(|&(order, layer)| (layer.slot, order));
        Self {
            layers: found.into_iter().map(|(_, layer)| layer).collect(),
            islands,
        }
    }
}

impl LevelRenderer {
    /// Lists every layer whose slot shows in `holes`, through the part of the
    /// screen those holes cover.
    pub(super) fn list_vistas(
        &self,
        view: &View,
        (width, height): (u32, u32),
        holes: &PolyList,
        out: &mut Vec<LayerList>,
    ) {
        let layers = &self.vistas.layers;
        out.resize_with(layers.len(), LayerList::default);
        for (layer, list) in layers.iter().zip(out.iter_mut()) {
            list.cam = None;
            let Some(window) = holes.bounds_of(u32::from(layer.slot)) else {
                continue;
            };
            let eye = View {
                pos: layer.pos + (view.pos - layer.centre) * layer.parallax,
                orientation: view.orientation,
            };
            let Some(cam) = eye.camera(self.cfg.fov_y, NEAR, FAR_VISTA, width, height) else {
                continue;
            };
            let islands = &self.vistas.islands;
            self.scene
                .list_layer(&cam, islands, layer.island, window, &mut list.faces);
            list.cam = Some(cam);
        }
    }

    /// Marks the holes in the rows `fb` holds, then draws each slot that shows.
    pub(super) fn fill_vistas(
        &self,
        fb: &mut Framebuffer,
        holes: &PolyList,
        lists: &[LayerList],
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
                let st = self.fill_layer(fb, layer, list);
                total.pixels += st.pixels;
                total.spans += st.spans;
            }
            fb.close_view();
        }
        total
    }

    /// Lightmap and texture only: a layer is far away, and neither the player's
    /// lamps nor their highlights reach it.
    fn fill_layer(&self, fb: &mut Framebuffer, layer: &Layer, list: &LayerList) -> DrawStats {
        let Some(cam) = list.cam.as_ref() else {
            return DrawStats::default();
        };
        let eye = eye_of(cam);
        self.scene
            .fill_frame(fb, &list.faces, None, &mut |i, face| {
                self.face_shader(i, face, eye, Quality::Low, &[], layer.fog)
            })
    }
}
