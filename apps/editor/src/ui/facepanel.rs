//! The "Face / texture" section of the properties column: a material
//! browser and Hammer's Face Edit fields for the faces being edited.

use mscent_render::Texture;
use mscent_world::texinfo::Justify;
use xui::Image;
use xui::backend::WidgetId;
use xui::prelude::*;
use xui::widget::{CheckBox, GroupBox, HasText};

use super::Msg;
use super::toolbar::IconButton;
use crate::model::EditorState;
use crate::model::facetex::FaceEdit;

/// Side of a material preview, in dip.
const THUMB: i32 = 40;
/// Texels per side of a preview image.
const PREVIEW: i32 = 40;

/// A message from the face section.
#[derive(Clone, Debug, PartialEq)]
pub enum FaceMsg {
    Edit(FaceEdit),
    /// Fit with the tile counts typed in the panel.
    Fit,
    /// Remember the first target face's mapping.
    Copy,
    /// Apply the remembered mapping to the target faces.
    Paste,
}

fn edit(e: FaceEdit) -> Msg {
    Msg::Face(FaceMsg::Edit(e))
}

/// Handles to every widget of the section.
#[derive(Default)]
pub struct FacePanel {
    previews: Vec<(String, Image)>,
    group: Handle<GroupBox<Msg>>,
    summary: Handle<Label<Msg>>,
    thumbs: Vec<Handle<IconButton<Msg>>>,
    scale: [Handle<NumberField<Msg>>; 2],
    shift: [Handle<NumberField<Msg>>; 2],
    rotation: Handle<NumberField<Msg>>,
    tiles: [Handle<NumberField<Msg>>; 2],
    follow: Handle<CheckBox<Msg>>,
    info: [Handle<Label<Msg>>; 2],
}

/// A square RGBA preview of a texture.
fn preview_of(t: &Texture) -> Option<Image> {
    let mut rgba = Vec::with_capacity((PREVIEW * PREVIEW * 4) as usize);
    for y in 0..PREVIEW {
        for x in 0..PREVIEW {
            let p = t.texel(
                x * t.width() as i32 / PREVIEW,
                y * t.height() as i32 / PREVIEW,
            );
            rgba.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8, 255]);
        }
    }
    Image::from_rgba(PREVIEW as u32, PREVIEW as u32, rgba).ok()
}

fn number(
    handle: &Handle<NumberField<Msg>>,
    (min, max, step): (f64, f64, f64),
    make: impl Fn(f64) -> Msg + 'static,
) -> Build<NumberField<Msg>, Msg> {
    number_field(min, max, step)
        .then(move |f| f.on_commit(move |v| Some(make(v))))
        .bind(handle)
}

fn small(text: &str, msg: Msg) -> Build<Button<Msg>, Msg> {
    button(text).on_click(msg)
}

fn round(v: f32) -> f64 {
    (f64::from(v) * 1.0e4).round() / 1.0e4
}

impl FacePanel {
    /// Reads the textures for the previews and returns their texel sizes,
    /// by material id. Without textures the browser is empty.
    pub fn load_previews(&mut self) -> Vec<(u32, u32)> {
        let Ok(table) = miniscent_game::assets::load_materials(None) else {
            return Vec::new();
        };
        let mut sizes = Vec::new();
        for i in 0..table.len() as u16 {
            let Some(m) = table.get(mscent_render::MaterialId(i)) else {
                continue;
            };
            sizes.push((m.texture.width(), m.texture.height()));
            if let Some(img) = preview_of(&m.texture) {
                self.previews.push((m.name.clone(), img));
            }
        }
        self.thumbs = self.previews.iter().map(|_| Handle::new()).collect();
        sizes
    }

    /// Whether `id` is one of the section's text fields.
    pub fn owns(&self, id: WidgetId) -> bool {
        [&self.scale[..], &self.shift, &self.tiles]
            .into_iter()
            .flatten()
            .chain([&self.rotation])
            .any(|h| h.get().id() == id)
    }

    pub fn layout(&self) -> impl IntoEntry<Msg> + use<> {
        let mut thumbs = wrap().gap(4);
        for (i, ((name, image), handle)) in self.previews.iter().zip(&self.thumbs).enumerate() {
            let (image, tip) = (image.clone(), format!("{name} (material {i})"));
            let msg = edit(FaceEdit::Material(i as u16));
            thumbs = thumbs.child(
                build(move |ui| IconButton::picture(ui, image, &tip, msg, THUMB)).bind(handle),
            );
        }
        let scale_range = (-1000.0, 1000.0, 0.0625);
        let shift_range = (-65_536.0, 65_536.0, 1.0);
        let fields = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Scale X"))
            .child(number(&self.scale[0], scale_range, |v| {
                edit(FaceEdit::ScaleU(v as f32))
            }))
            .child(label("Scale Y"))
            .child(number(&self.scale[1], scale_range, |v| {
                edit(FaceEdit::ScaleV(v as f32))
            }))
            .child(label("Shift X"))
            .child(number(&self.shift[0], shift_range, |v| {
                edit(FaceEdit::ShiftU(v as f32))
            }))
            .child(label("Shift Y"))
            .child(number(&self.shift[1], shift_range, |v| {
                edit(FaceEdit::ShiftV(v as f32))
            }))
            .child(label("Rotation"))
            .child(number(&self.rotation, (-360.0, 360.0, 15.0), |v| {
                edit(FaceEdit::Rotation(v as f32))
            }));
        let fit = grid([Track::Auto, Track::Fill(1), Track::Fill(1), Track::Auto])
            .gap(6)
            .child(label("Tiles"))
            .child(number_field(1.0, 256.0, 1.0).bind(&self.tiles[0]))
            .child(number_field(1.0, 256.0, 1.0).bind(&self.tiles[1]))
            .child(
                small("Fit", Msg::Face(FaceMsg::Fit)).tooltip("Stretch the texture over the face"),
            );
        let justify = |text: &str, how| small(text, edit(FaceEdit::Justify(how)));
        let buttons = wrap()
            .gap(4)
            .child(justify("Left", Justify::Left))
            .child(justify("Right", Justify::Right))
            .child(justify("Top", Justify::Top))
            .child(justify("Bottom", Justify::Bottom))
            .child(justify("Center", Justify::Center))
            .child(small("Align face", edit(FaceEdit::AlignFace)))
            .child(small("Align world", edit(FaceEdit::AlignWorld)))
            .child(small("Flip U", edit(FaceEdit::FlipU)))
            .child(small("Flip V", edit(FaceEdit::FlipV)))
            .child(
                small("Copy", Msg::Face(FaceMsg::Copy))
                    .tooltip("Copy this face's texture (Alt + click)"),
            )
            .child(
                small("Paste", Msg::Face(FaceMsg::Paste))
                    .tooltip("Apply the copied texture (Alt + right click)"),
            );
        group(
            "Face / texture",
            column()
                .gap(6)
                .child(label("No face").bind(&self.summary))
                .child(thumbs)
                .child(fields)
                .child(fit)
                .child(buttons)
                .child(
                    checkbox("Texture moves with brush")
                        .on_toggle(|on| edit(FaceEdit::FollowBrush(on)))
                        .bind(&self.follow),
                )
                .child(label("").bind(&self.info[0]))
                .child(label("").bind(&self.info[1])),
        )
        .bind(&self.group)
    }

    /// The tile counts typed for Fit.
    pub fn tiles(&self) -> (f32, f32) {
        (
            self.tiles[0].get().value() as f32,
            self.tiles[1].get().value() as f32,
        )
    }

    /// Shows the target faces; `show` hides the whole section.
    pub fn sync(&self, st: &EditorState, show: impl Fn(WidgetId, bool)) {
        let view = st.face_view();
        show(self.group.get().id(), view.is_some());
        let Some(v) = view else { return };
        let t = &v.info;
        let faces = if v.count == 1 {
            "1 face".to_string()
        } else {
            format!("{} faces", v.count)
        };
        let name = self
            .previews
            .get(usize::from(t.material))
            .map_or("?", |p| p.0.as_str());
        self.summary.get().set_text(&format!("{faces}: {name}"));
        for (h, i) in self.thumbs.iter().zip(0u16..) {
            h.get().set_checked(i == t.material);
        }
        for (h, value) in self.scale.iter().zip([t.u_scale, t.v_scale]) {
            h.get().set_value(round(value));
        }
        for (h, value) in self.shift.iter().zip([t.u_shift, t.v_shift]) {
            h.get().set_value(round(value));
        }
        self.rotation.get().set_value(round(t.rotation_deg));
        self.follow.get().set_checked(!t.lock_to_world);
        self.info[0]
            .get()
            .set_text(&format!("Texture {} x {} texels", v.texture.0, v.texture.1));
        self.info[1]
            .get()
            .set_text(&format!("Tile = {:.4} x {:.4} units", v.tile.0, v.tile.1));
    }
}
