//! A frame's vistas for a renderer that lists and draws its own faces: each
//! layer's camera, fog, pan and faces, chosen as the software frame chooses them,
//! so that the two cannot disagree about what a viewport shows.

use super::{VISTA_VOXEL_TRIS, clean_time, pan_texels};
use crate::camera::Camera;
use crate::level::{LevelAssets, View};
use crate::scene::HoleList;
use crate::shade::Fog;
use crate::voxel::list::VoxelList;

/// One layer of a vista in one frame.
#[derive(Clone, Debug)]
pub struct VistaView {
    /// Which of the level's layers this is, in the order software keeps them.
    pub layer: usize,
    pub slot: u8,
    pub cam: Camera,
    /// The layer's own fog, not the level's.
    pub fog: Option<Fog>,
    /// Texels its self-lit faces have scrolled along each texture axis.
    pub pan: [f32; 2],
    /// The part of the screen its slot's viewport faces cover, `[x0, y0, x1, y1]`.
    pub window: [f32; 4],
    /// Its faces in view, front to back, as indices into `Scene::faces`.
    pub faces: Vec<u32>,
    /// The voxel chunks of its room's leaves in view.
    pub chunks: Vec<u32>,
}

impl LevelAssets {
    /// How many vista layers the level has, over every slot.
    pub fn vista_layer_count(&self) -> usize {
        self.vistas.layers.len()
    }

    /// Replaces `out` with the layers of every slot that shows in `holes`, by
    /// slot and far to near within one, as the software renderer draws them at
    /// `time` seconds for a player at `view`, drawing `size`. A layer whose eye
    /// makes no camera is left out, as software leaves it. Returns how many
    /// faces were listed.
    pub fn vista_views(
        &self,
        view: &View,
        size: (u32, u32),
        holes: &HoleList,
        time: f32,
        out: &mut Vec<VistaView>,
    ) -> u32 {
        out.clear();
        let time = clean_time(time);
        let mut listed = 0u32;
        for (index, layer) in self.vistas.layers.iter().enumerate() {
            let Some(window) = holes.window(layer.slot) else {
                continue;
            };
            let Some(cam) = self.layer_camera(layer, view, time, size) else {
                continue;
            };
            let mut chunks = Vec::new();
            let islands = &self.vistas.islands;
            let lists = (None, &mut chunks);
            let st = (self.scene).list_layer(&cam, islands, layer.island, window, lists, None);
            listed = listed.saturating_add(u32::try_from(st.drawn.len()).unwrap_or(u32::MAX));
            out.push(VistaView {
                layer: index,
                slot: layer.slot,
                cam,
                fog: layer.fog,
                pan: pan_texels(layer.pan, time).unwrap_or([0.0; 2]),
                window,
                faces: st.drawn,
                chunks,
            });
        }
        listed
    }

    /// Lists the voxels of each of `views` into the entry of `lists` for its
    /// layer, kept from frame to frame as software keeps them, with the budgets
    /// software gives them. Returns the triangles listed.
    pub fn vista_voxels(&self, views: &[VistaView], lists: &mut Vec<VoxelList>) -> u32 {
        lists.resize_with(self.vistas.layers.len(), VoxelList::unprojected);
        let mut left = VISTA_VOXEL_TRIS;
        for view in views {
            let Some(list) = lists.get_mut(view.layer) else {
                continue;
            };
            list.candidates.clear();
            list.candidates.extend_from_slice(&view.chunks);
            self.list_layer_voxels(list, &view.cam, &mut left);
        }
        VISTA_VOXEL_TRIS - left
    }
}
