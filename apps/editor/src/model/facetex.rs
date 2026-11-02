//! Face selection and Hammer-style texture editing.
//!
//! The faces an edit applies to ([`EditorState::target_faces`]) are the ones
//! picked in the 3D view, or every face of the selected brush when none is
//! picked. Each edit is one undo step; consecutive edits of the same value
//! field on the same faces merge into one, so dragging a spinner undoes in a
//! single go. None of this touches the lightmap: it depends on geometry
//! only, so a level with new mappings re-uses its cached bake.

use std::mem::{Discriminant, discriminant};

use mscent_math::{Polygon, Vec3d};
use mscent_world::TexInfo;
use mscent_world::texinfo::Justify;

use super::command::Command;
use super::pick::{self, FaceHit, FaceRef, Ray, face_polygon};
use super::state::{EditorState, Selection};

/// Texel size assumed for a material the editor knows nothing about.
const DEFAULT_TEXTURE: (u32, u32) = (128, 128);

/// One change to the mapping of the target faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FaceEdit {
    Material(u16),
    /// World units per texel, along the texture's u axis.
    ScaleU(f32),
    ScaleV(f32),
    /// Texels.
    ShiftU(f32),
    ShiftV(f32),
    /// Degrees.
    Rotation(f32),
    /// Stretch so the face spans this many repeats of the texture.
    Fit {
        tiles_u: f32,
        tiles_v: f32,
    },
    Justify(Justify),
    /// Axes along the face's edges.
    AlignFace,
    /// Axes from the world, by the face's direction.
    AlignWorld,
    FlipU,
    FlipV,
    /// Hammer's texture lock: the texture moves with the brush.
    FollowBrush(bool),
}

impl FaceEdit {
    /// Value fields merge into one undo step when edited in a row.
    fn mergeable(self) -> bool {
        matches!(
            self,
            FaceEdit::ScaleU(_)
                | FaceEdit::ScaleV(_)
                | FaceEdit::ShiftU(_)
                | FaceEdit::ShiftV(_)
                | FaceEdit::Rotation(_)
        )
    }
}

/// What the last face edit was, to decide whether the next one merges.
#[derive(Clone, Debug, PartialEq)]
pub struct MergeKey {
    kind: Discriminant<FaceEdit>,
    faces: Vec<FaceRef>,
    depth: usize,
}

/// A copied mapping: Alt + left click.
#[derive(Clone, Debug)]
pub struct FaceClip {
    texinfo: TexInfo,
    polygon: Polygon,
}

/// What the face panel shows about the first target face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceView {
    pub info: TexInfo,
    /// How many faces an edit applies to.
    pub count: usize,
    pub texture: (u32, u32),
    /// World size of one repeat of the texture along u and v.
    pub tile: (f64, f64),
}

/// Applies `edit` to one face's mapping; whether anything changed.
fn edit_texinfo(
    t: &mut TexInfo,
    edit: FaceEdit,
    face: &Polygon,
    normal: Vec3d,
    size: (u32, u32),
) -> bool {
    let before = *t;
    match edit {
        FaceEdit::Material(m) => t.material = m,
        FaceEdit::ScaleU(s) => t.set_scale(s, t.v_scale),
        FaceEdit::ScaleV(s) => t.set_scale(t.u_scale, s),
        FaceEdit::ShiftU(s) => t.shift_by(s - t.u_shift, 0.0),
        FaceEdit::ShiftV(s) => t.shift_by(0.0, s - t.v_shift),
        FaceEdit::Rotation(d) => t.rotate(d - t.rotation_deg),
        FaceEdit::Fit { tiles_u, tiles_v } => {
            t.fit(face, size, tiles_u, tiles_v);
        }
        FaceEdit::Justify(how) => {
            t.justify(face, size, how);
        }
        FaceEdit::AlignFace => {
            t.align_to_face(face, normal);
        }
        FaceEdit::AlignWorld => t.align_to_world(normal),
        FaceEdit::FlipU => t.flip_u(),
        FaceEdit::FlipV => t.flip_v(),
        FaceEdit::FollowBrush(on) => t.lock_to_world = !on,
    }
    *t != before
}

impl EditorState {
    /// The texel size of a material.
    pub fn texture_size(&self, material: u16) -> (u32, u32) {
        self.texture_sizes
            .get(usize::from(material))
            .copied()
            .unwrap_or(DEFAULT_TEXTURE)
    }

    /// Faces the panel edits: the picked ones that still exist, else every
    /// face of the selected brush.
    pub fn target_faces(&self) -> Vec<FaceRef> {
        // A volume has no faces to texture.
        let planes = |id| {
            let brush = self.doc.brush(id).filter(|b| !b.is_volume());
            brush.map_or(0, |b| b.full_texinfos().len())
        };
        let picked: Vec<FaceRef> = self
            .faces
            .iter()
            .copied()
            .filter(|f| f.plane < planes(f.brush))
            .collect();
        if !picked.is_empty() {
            return picked;
        }
        match self.selection {
            Some(Selection::Brush(id)) => (0..planes(id))
                .map(|plane| FaceRef { brush: id, plane })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The mapping of a face (defaults filled in).
    pub fn face_texinfo(&self, face: FaceRef) -> Option<TexInfo> {
        self.doc
            .brush(face.brush)?
            .full_texinfos()
            .get(face.plane)
            .copied()
    }

    /// The first target face's mapping and sizes, for the panel.
    pub fn face_view(&self) -> Option<FaceView> {
        let targets = self.target_faces();
        let info = self.face_texinfo(*targets.first()?)?;
        let texture = self.texture_size(info.material);
        Some(FaceView {
            info,
            count: targets.len(),
            texture,
            tile: (
                f64::from(texture.0) * f64::from(info.u_scale.abs()),
                f64::from(texture.1) * f64::from(info.v_scale.abs()),
            ),
        })
    }

    /// Picks the face under `ray`. Without `additive` it becomes the only
    /// picked face; with it (Ctrl) it is toggled in the set. Picking nothing
    /// without `additive` clears the set. Returns the hit.
    pub fn pick_face_at(&mut self, ray: &Ray, additive: bool) -> Option<FaceHit> {
        let hit = pick::pick_visible_face(&self.doc, ray);
        match (hit, additive) {
            (Some(h), false) => {
                self.faces = vec![h.face];
                self.selection = Some(Selection::Brush(h.face.brush));
            }
            (Some(h), true) => {
                if let Some(i) = self.faces.iter().position(|f| *f == h.face) {
                    self.faces.remove(i);
                } else {
                    self.faces.push(h.face);
                }
                self.selection = Some(Selection::Brush(h.face.brush));
            }
            (None, false) => self.faces.clear(),
            (None, true) => {}
        }
        self.message = match (hit, self.faces.len()) {
            (None, 0) => "No face there".into(),
            (_, n) => format!("{n} face{} picked", if n == 1 { "" } else { "s" }),
        };
        hit
    }

    /// Selects the brush whose visible surface is under `ray`, as the Select
    /// tool does from the 3D view. Hitting nothing clears the selection.
    pub fn select_brush_at(&mut self, ray: &Ray) -> Option<FaceHit> {
        let hit = pick::pick_visible_face(&self.doc, ray);
        self.selection = hit.map(|h| Selection::Brush(h.face.brush));
        hit
    }

    /// Applies `edit` to every target face as one undo step (merged with the
    /// previous one for a repeated value edit). Whether anything changed.
    pub fn apply_face_edit(&mut self, edit: FaceEdit) -> bool {
        let faces = self.target_faces();
        let mut cmds = Vec::new();
        let mut done: Vec<u32> = Vec::new();
        for f in &faces {
            if done.contains(&f.brush) {
                continue;
            }
            done.push(f.brush);
            let Some(before) = self.doc.brush(f.brush).cloned() else {
                continue;
            };
            let mut after = before.clone();
            let mut all = after.full_texinfos();
            let mut changed = false;
            for g in faces.iter().filter(|g| g.brush == f.brush) {
                let Some((poly, normal)) = face_polygon(&self.doc, *g) else {
                    continue;
                };
                let Some(t) = all.get_mut(g.plane) else {
                    continue;
                };
                let size = self.texture_size(t.material);
                changed |= edit_texinfo(t, edit, &poly, normal, size);
            }
            if !changed {
                continue;
            }
            if let Some(first) = all.first().map(|t| t.material)
                && all.iter().all(|t| t.material == first)
            {
                after.material = first;
            }
            after.texinfos = all;
            cmds.push(Command::EditBrush { before, after });
        }
        if cmds.is_empty() {
            return false;
        }
        let kind = discriminant(&edit);
        let merge = edit.mergeable()
            && self.face_key.as_ref()
                == Some(&MergeKey {
                    kind,
                    faces: faces.clone(),
                    depth: self.history.depth(),
                });
        self.history
            .run_merging(&mut self.doc, Command::Batch(cmds), merge);
        self.face_key = Some(MergeKey {
            kind,
            faces,
            depth: self.history.depth(),
        });
        true
    }

    /// Remembers the mapping of `face` for [`EditorState::paste_onto`].
    pub fn copy_face(&mut self, face: FaceRef) -> bool {
        let (Some(texinfo), Some((polygon, _))) =
            (self.face_texinfo(face), face_polygon(&self.doc, face))
        else {
            return false;
        };
        self.face_clip = Some(FaceClip { texinfo, polygon });
        self.message = "Copied the face's texture".into();
        true
    }

    /// Gives `faces` the copied material and mapping. Where a face shares an
    /// edge with the copied one the texture continues across it (Hammer's
    /// apply with alignment); otherwise scale, shift and rotation are
    /// copied and the face keeps its own axes. One undo step.
    pub fn paste_onto(&mut self, faces: &[FaceRef]) -> bool {
        let Some(clip) = self.face_clip.clone() else {
            self.message = "Copy a face first (Alt + left click)".into();
            return false;
        };
        let mut cmds = Vec::new();
        let mut done: Vec<u32> = Vec::new();
        for f in faces {
            if done.contains(&f.brush) {
                continue;
            }
            done.push(f.brush);
            let Some(before) = self.doc.brush(f.brush).cloned() else {
                continue;
            };
            let mut after = before.clone();
            let mut all = after.full_texinfos();
            for g in faces.iter().filter(|g| g.brush == f.brush) {
                let (Some((poly, _)), Some(slot)) =
                    (face_polygon(&self.doc, *g), all.get_mut(g.plane))
                else {
                    continue;
                };
                *slot = clip
                    .texinfo
                    .copy_across_edge(&clip.polygon, &poly)
                    .unwrap_or(TexInfo {
                        u_axis: slot.u_axis,
                        v_axis: slot.v_axis,
                        ..clip.texinfo
                    });
            }
            after.texinfos = all;
            if after != before {
                cmds.push(Command::EditBrush { before, after });
            }
        }
        if cmds.is_empty() {
            return false;
        }
        self.history.run(&mut self.doc, Command::Batch(cmds));
        self.face_key = None;
        self.message = "Applied the copied texture".into();
        true
    }

    /// The faces to outline in the 3D view: the targets while the face tool
    /// is active, as lists of corners.
    pub fn highlighted_faces(&self) -> Vec<Vec<mscent_math::Vec3>> {
        if self.tool != super::state::Tool::Face {
            return Vec::new();
        }
        self.target_faces()
            .into_iter()
            .filter_map(|f| face_polygon(&self.doc, f))
            .map(|(poly, _)| poly.vertices().iter().map(|v| v.to_f32()).collect())
            .collect()
    }
}
