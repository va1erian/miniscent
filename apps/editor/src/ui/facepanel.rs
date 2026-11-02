//! The "Face / texture" section of the properties column: a material
//! browser and Hammer's Face Edit fields for the faces being edited.

use mscent_render::{MaterialTable, Texture};
use mscent_world::texinfo::Justify;
use xui::Image;
use xui::backend::WidgetId;
use xui::prelude::*;
use xui::widget::{CheckBox, GroupBox, HasText};

use super::Msg;
use super::app::cmd;
use super::props::Mount;
use super::toolbar::IconButton;
use crate::model::EditorState;
use crate::model::facetex::{DEFAULT_TEXTURE, FaceEdit, FaceView};

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

/// A material in the browser. Its place in the list is not its id: retired ids
/// have no place.
struct Preview {
    id: u16,
    name: String,
    image: Image,
}

/// Handles to every widget of the section.
#[derive(Default)]
pub struct FacePanel {
    previews: Vec<Preview>,
    /// Holds the thumbnails, which are made once per list.
    thumb_host: Handle<xui::widget::Panel<Msg>>,
    group: Handle<GroupBox<Msg>>,
    summary: Handle<Label<Msg>>,
    thumbs: Vec<Handle<IconButton<Msg>>>,
    scale: [Handle<NumberField<Msg>>; 2],
    shift: [Handle<NumberField<Msg>>; 2],
    rotation: Handle<NumberField<Msg>>,
    tiles: [Handle<NumberField<Msg>>; 2],
    follow: Handle<CheckBox<Msg>>,
    info: Handle<Label<Msg>>,
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

/// The material browser, in a layout of its own to mount in its container.
fn thumbnails(
    previews: &[Preview],
    handles: &[Handle<IconButton<Msg>>],
    root: Option<Ui<Msg>>,
) -> Layout<Msg> {
    let mut thumbs = wrap().gap(4);
    for (preview, handle) in previews.iter().zip(handles) {
        let tip = format!("{} (material {})", preview.name, preview.id);
        let (image, msg) = (preview.image.clone(), edit(FaceEdit::Material(preview.id)));
        let root = root.clone();
        thumbs = thumbs.child(
            build(move |ui| {
                IconButton::picture(ui, root.as_ref().unwrap_or(ui), image, &tip, msg, THUMB)
            })
            .bind(handle),
        );
    }
    thumbs
}

fn round(v: f32) -> f64 {
    (f64::from(v) * 1.0e4).round() / 1.0e4
}

impl FacePanel {
    /// The number of thumbnails, for tests.
    #[cfg(test)]
    pub fn thumb_count(&self) -> usize {
        self.thumbs.len()
    }

    /// Makes the previews of `table`, remaking the thumbnails if they exist, and
    /// returns the texel sizes by material id: a retired id, which draws the grey
    /// fallback, has the size the scene gives that. Without textures the browser
    /// is empty. `root` is as in [`Self::layout`]. After an error the old
    /// thumbnails work as they did.
    pub(super) fn show_materials(
        &mut self,
        table: Option<&MaterialTable>,
        root: Option<&Ui<Msg>>,
        mount: &Mount<'_>,
    ) -> xui::backend::Result<Vec<(u32, u32)>> {
        let mut sizes = vec![DEFAULT_TEXTURE; table.map_or(0, MaterialTable::len)];
        let mut previews = Vec::new();
        for (id, m) in table.iter().flat_map(|t| t.iter()) {
            sizes[usize::from(id.0)] = (m.texture.width(), m.texture.height());
            if let Some(image) = preview_of(&m.texture) {
                let (id, name) = (id.0, m.name.clone());
                previews.push(Preview { id, name, image });
            }
        }
        let thumbs: Vec<_> = previews.iter().map(|_| Handle::new()).collect();
        if let Some(host) = self.thumb_host.try_get() {
            mount(&host, thumbnails(&previews, &thumbs, root.cloned()))?;
        }
        self.previews = previews;
        self.thumbs = thumbs;
        Ok(sizes)
    }

    /// Whether `id` is one of the section's text fields.
    pub fn owns(&self, id: WidgetId) -> bool {
        [&self.scale[..], &self.shift, &self.tiles]
            .into_iter()
            .flatten()
            .chain([&self.rotation])
            .any(|h| h.get().id() == id)
    }

    /// `root` is the window-level handle the thumbnails' tooltips are made
    /// through, so they are not clipped to this panel.
    pub fn layout(&self, root: Option<Ui<Msg>>) -> impl IntoEntry<Msg> + use<> {
        let thumbs = thumbnails(&self.previews, &self.thumbs, root);
        let thumbs = panel(thumbs).plain().bind(&self.thumb_host);
        let scale_range = (-1000.0, 1000.0, 0.0625);
        let shift_range = (-65_536.0, 65_536.0, 1.0);
        let pair = |label_text: &str,
                    handles: &[Handle<NumberField<Msg>>; 2],
                    range: (f64, f64, f64),
                    make: [fn(f32) -> FaceEdit; 2]| {
            (
                label(label_text),
                number(&handles[0], range, move |v| edit(make[0](v as f32))),
                number(&handles[1], range, move |v| edit(make[1](v as f32))),
            )
        };
        // The U and V fields of one setting share a row.
        let (l1, u1, v1) = pair(
            "Scale",
            &self.scale,
            scale_range,
            [FaceEdit::ScaleU, FaceEdit::ScaleV],
        );
        let (l2, u2, v2) = pair(
            "Shift",
            &self.shift,
            shift_range,
            [FaceEdit::ShiftU, FaceEdit::ShiftV],
        );
        let fields = grid([Track::Auto, Track::Fill(1), Track::Fill(1)])
            .gap(6)
            .child(l1)
            .child(u1)
            .child(v1)
            .child(l2)
            .child(u2)
            .child(v2)
            .child(label("Rotation"))
            .child(number(&self.rotation, (-360.0, 360.0, 15.0), |v| {
                edit(FaceEdit::Rotation(v as f32))
            }))
            .child(label("degrees"));
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
                small("Extrude out", Msg::Menu(cmd::EXTRUDE))
                    .tooltip("Push the picked faces out by one grid step (Ctrl+E)"),
            )
            .child(
                small("Extrude in", Msg::Menu(cmd::EXTRUDE_IN))
                    .tooltip("Pull the picked faces in by one grid step (Ctrl+Shift+E)"),
            )
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
                .child(label("").bind(&self.info)),
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
        self.sync_lists(&v);
        let t = &v.info;
        for (h, value) in self.scale.iter().zip([t.u_scale, t.v_scale]) {
            h.get().set_value(round(value));
        }
        for (h, value) in self.shift.iter().zip([t.u_shift, t.v_shift]) {
            h.get().set_value(round(value));
        }
        self.rotation.get().set_value(round(t.rotation_deg));
        self.follow.get().set_checked(!t.lock_to_world);
    }

    /// Shows the target faces in what depends on the material table, leaving the
    /// fields alone.
    pub(super) fn sync_lists(&self, v: &FaceView) {
        let t = &v.info;
        let faces = if v.count == 1 {
            "1 face".to_string()
        } else {
            format!("{} faces", v.count)
        };
        let name = self
            .previews
            .iter()
            .find(|p| p.id == t.material)
            .map_or("?", |p| p.name.as_str());
        self.summary.get().set_text(&format!("{faces}: {name}"));
        for (h, p) in self.thumbs.iter().zip(&self.previews) {
            h.get().set_checked(p.id == t.material);
        }
        self.info.get().set_text(&format!(
            "{} x {} texels, tile {:.2} x {:.2} units",
            v.texture.0, v.texture.1, v.tile.0, v.tile.1
        ));
    }
}
