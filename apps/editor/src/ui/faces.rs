//! The editor's side of face texturing: messages from the face panel and
//! clicks on the 3D picture.

use mscent_render::BuildConfig;

use super::app::Editor;
use super::facepanel::FaceMsg;
use super::view3d::Click;
use crate::model::Tool;
use crate::model::facetex::FaceEdit;
use crate::model::pick::{self, view_ray};

impl Editor {
    /// A button or field of the face section.
    pub(super) fn face_message(&mut self, msg: FaceMsg) {
        let mut st = self.state.borrow_mut();
        match msg {
            FaceMsg::Edit(edit) => {
                st.apply_face_edit(edit);
            }
            FaceMsg::Fit => {
                let (tiles_u, tiles_v) = self.panel.face.tiles();
                if !st.apply_face_edit(FaceEdit::Fit { tiles_u, tiles_v }) {
                    st.message = "Fit changed nothing".into();
                }
            }
            FaceMsg::Copy => {
                if let Some(face) = st.target_faces().first().copied() {
                    st.copy_face(face);
                }
            }
            FaceMsg::Paste => {
                let faces = st.target_faces();
                st.paste_onto(&faces);
            }
        }
    }

    /// A press on the 3D picture. The Select tool selects the brush under it;
    /// the face tool picks, copies (Alt + left) or pastes (Alt + right).
    pub(super) fn pick_click(&mut self, click: Click) {
        let view = self.studio.fly_camera().view();
        let fov = BuildConfig::default().fov_y;
        let Some(ray) = view_ray(&view, fov, click.nx, click.ny, click.aspect) else {
            return;
        };
        let mut st = self.state.borrow_mut();
        if st.tool == Tool::Select {
            if !click.right {
                st.select_brush_at(&ray);
            }
            return;
        }
        if st.tool != Tool::Face {
            return;
        }
        if click.alt {
            let hit = pick::pick_face(&st.doc, &ray);
            match (hit, click.right) {
                (Some(h), false) => {
                    st.copy_face(h.face);
                }
                (Some(h), true) => {
                    st.paste_onto(&[h.face]);
                }
                (None, _) => st.message = "No face there".into(),
            }
        } else {
            st.pick_face_at(&ray, click.ctrl);
        }
    }
}
