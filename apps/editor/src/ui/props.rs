//! The right-hand properties panel: layout, syncing from the selection and
//! applying edits back through the model.
// Several parallel per-axis arrays are indexed by the same axis number.
#![allow(clippy::needless_range_loop)]

use mscent_world::{Entity, Op, materials::NAMES};
use xui::backend::WidgetId;
use xui::prelude::*;
use xui::widget::{Edit, GroupBox, HasText};

use super::Msg;
use super::facepanel::FacePanel;
use super::voxelpanel::{VoxelPanel, VoxelProp, apply as apply_voxel};
use crate::model::entity::{entity_name, params, pos, set_param, with_pos};
use crate::model::{EditorState, Shape, Tool};

/// A field edit made in the panel.
#[derive(Clone, Debug, PartialEq)]
pub enum Prop {
    Op(usize),
    Material(usize),
    Min(usize, f64),
    Max(usize, f64),
    Pos(usize, f64),
    Param(usize, f64),
    /// What is typed in the Name field.
    NameText(String),
    /// What is typed in the Tags field.
    TagsText(String),
    /// The Apply button: the name and tags become the selection's.
    ApplyNames,
    /// A change in the Voxel tool's section.
    Voxel(VoxelProp),
}

const AXES: [&str; 3] = ["X", "Y", "Z"];
/// Most kind-specific fields any entity has.
const PARAMS: usize = 5;

/// Handles to every widget that shows selection data.
#[derive(Default)]
pub struct Panel {
    ui: Option<Ui<Msg>>,
    brush_group: Handle<GroupBox<Msg>>,
    entity_group: Handle<GroupBox<Msg>>,
    what: Handle<Label<Msg>>,
    op: Handle<ComboBox<Msg>>,
    material: Handle<ComboBox<Msg>>,
    min: [Handle<NumberField<Msg>>; 3],
    max: [Handle<NumberField<Msg>>; 3],
    pos: [Handle<NumberField<Msg>>; 3],
    param_labels: [Handle<Label<Msg>>; PARAMS],
    params: [Handle<NumberField<Msg>>; PARAMS],
    names_group: Handle<GroupBox<Msg>>,
    name: Handle<Edit<Msg>>,
    tags: Handle<Edit<Msg>>,
    uses: Handle<Label<Msg>>,
    pub voxel: VoxelPanel,
    /// What the name and tag fields last showed, so typing is not overwritten.
    shown_names: std::cell::RefCell<(String, String)>,
    pub face: FacePanel,
}

fn field(
    handle: &Handle<NumberField<Msg>>,
    make: impl Fn(f64) -> Prop + 'static,
) -> Build<NumberField<Msg>, Msg> {
    number_field(-4096.0, 4096.0, 1.0)
        .then(move |f| f.on_commit(move |v| Some(Msg::Prop(make(v)))))
        .bind(handle)
}

impl Panel {
    /// The Min X field, for tests.
    #[cfg(test)]
    pub fn min_x(&self) -> std::rc::Rc<NumberField<Msg>> {
        self.min[0].get()
    }

    /// Whether `id` is one of the panel's fields or lists (which take typed
    /// keys, so the editor's single-key shortcuts leave them alone).
    pub fn owns(&self, id: WidgetId) -> bool {
        let numbers = [&self.min[..], &self.max, &self.pos, &self.params]
            .into_iter()
            .flatten()
            .any(|h| h.get().id() == id);
        let names = self.name.get().id() == id || self.tags.get().id() == id;
        numbers
            || names
            || self.voxel.owns(id)
            || self.face.owns(id)
            || self.op.get().id() == id
            || self.material.get().id() == id
    }

    /// Gives the panel the UI it needs to show and hide its sections.
    pub fn attach(&mut self, ui: &Ui<Msg>) {
        self.ui = Some(ui.clone());
    }

    fn show(&self, id: WidgetId, visible: bool) {
        if let Some(ui) = &self.ui {
            ui.set_visible(id, visible);
        }
    }

    pub fn layout(&self) -> impl IntoEntry<Msg> + use<> {
        let mut brush = grid([Track::Auto, Track::Fill(1)]).gap(6);
        brush = brush
            .child(label("Operation"))
            .child(
                combo_box(&["Add", "Subtract"])
                    .on_select(|i| Msg::Prop(Prop::Op(i)))
                    .bind(&self.op),
            )
            .child(label("Material"))
            .child(
                combo_box(&NAMES)
                    .on_select(|i| Msg::Prop(Prop::Material(i)))
                    .bind(&self.material),
            );
        // Min and Max take one row each (X, Y, Z side by side) so the
        // Face section below stays on screen.
        let mut bounds = grid([Track::Auto, Track::Fill(1), Track::Fill(1), Track::Fill(1)]).gap(4);
        bounds = bounds
            .child(label(""))
            .child(label("X"))
            .child(label("Y"))
            .child(label("Z"));
        bounds = bounds.child(label("Min"));
        for a in 0..3 {
            bounds = bounds.child(field(&self.min[a], move |v| Prop::Min(a, v)));
        }
        bounds = bounds.child(label("Max"));
        for a in 0..3 {
            bounds = bounds.child(field(&self.max[a], move |v| Prop::Max(a, v)));
        }
        let brush = column().gap(6).child(brush).child(bounds);
        let mut ent = grid([Track::Auto, Track::Fill(1)]).gap(6);
        for a in 0..3 {
            ent = ent
                .child(label(format!("Pos {}", AXES[a])))
                .child(field(&self.pos[a], move |v| Prop::Pos(a, v)));
        }
        for i in 0..PARAMS {
            ent = ent
                .child(label("-").bind(&self.param_labels[i]))
                .child(field(&self.params[i], move |v| Prop::Param(i, v)));
        }
        let names = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Name"))
            .child(
                edit()
                    .then(|e| e.on_change(|t| Some(Msg::Prop(Prop::NameText(t.to_string())))))
                    .bind(&self.name),
            )
            .child(label("Tags"))
            .child(
                edit()
                    .then(|e| e.on_change(|t| Some(Msg::Prop(Prop::TagsText(t.to_string())))))
                    .bind(&self.tags),
            );
        let names = column().gap(6).children((
            names,
            button("Apply names").on_click(Msg::Prop(Prop::ApplyNames)),
            label("").bind(&self.uses),
        ));
        scroll(column().padding(8).gap(8).children((
            label("Nothing selected").bind(&self.what),
            group("Voxel tool", self.voxel.layout()).bind(&self.voxel.group),
            group("Script names", names).bind(&self.names_group),
            group("Brush", brush).bind(&self.brush_group),
            self.face.layout(self.ui.clone()),
            group("Entity", ent).bind(&self.entity_group),
        )))
    }

    /// The name and tag fields follow the selection, and leave typing alone.
    fn sync_names(&self, st: &mut EditorState) {
        let shown = st.sync_draft();
        if *self.shown_names.borrow() != shown {
            if self.name.get().text() != shown.0 {
                self.name.get().set_text(&shown.0);
            }
            if self.tags.get().text() != shown.1 {
                self.tags.get().set_text(&shown.1);
            }
            *self.shown_names.borrow_mut() = shown;
        }
        st.poll_script();
        self.show(self.names_group.get().id(), st.selection.is_some());
        self.uses.get().set_text(&st.script_uses());
    }

    /// Shows the selection in the fields and enables only the relevant ones.
    pub fn sync(&self, st: &mut EditorState) {
        self.sync_names(st);
        self.voxel.sync(st, |id, visible| self.show(id, visible));
        let brush = st.selected_brush();
        let ent = st.selected_entity();
        let text = match (brush, ent) {
            (Some(b), _) => {
                let index = st.doc.brush_index(b.id).map_or(0, |i| i + 1);
                let op = if b.op == Op::Add { "add" } else { "subtract" };
                let shape = match (b.is_volume(), b.is_box()) {
                    (true, _) => "voxel volume",
                    (_, true) => "box",
                    _ => "convex",
                };
                format!("Brush {index} of {} ({op} {shape})", st.doc.brushes().len())
            }
            (_, Some(e)) => entity_name(&e.entity).to_string(),
            _ => "Nothing selected".to_string(),
        };
        self.what.get().set_text(&text);
        // Only the section that applies is shown: no greyed placeholder
        // values for something that is not selected.
        self.show(
            self.brush_group.get().id(),
            brush.is_some() && st.tool != Tool::Face,
        );
        self.show(self.entity_group.get().id(), ent.is_some());
        self.face.sync(st, |id, visible| self.show(id, visible));

        let (lo, hi) = brush.map_or(([0.0; 3], [0.0; 3]), |b| b.bounds());
        let editable = brush.is_some_and(|b| b.is_box());
        self.op.get().set_enabled(brush.is_some());
        self.material.get().set_enabled(brush.is_some());
        if let Some(b) = brush {
            self.op.get().select(usize::from(b.op == Op::Subtract));
            self.material
                .get()
                .select((b.material as usize).min(NAMES.len() - 1));
        }
        for a in 0..3 {
            self.min[a].get().set_value(lo[a]);
            self.max[a].get().set_value(hi[a]);
            self.min[a].get().set_enabled(editable);
            self.max[a].get().set_enabled(editable);
        }
        let p = ent.map_or([0.0; 3], |e| pos(&e.entity));
        for a in 0..3 {
            self.pos[a].get().set_value(p[a]);
            self.pos[a].get().set_enabled(ent.is_some());
        }
        let list = ent.map(|e| params(&e.entity)).unwrap_or_default();
        for i in 0..PARAMS {
            let (name, value) = list.get(i).copied().unwrap_or(("-", 0.0));
            self.param_labels[i].get().set_text(name);
            self.params[i].get().set_value(value);
            self.params[i].get().set_enabled(i < list.len());
            self.show(self.param_labels[i].get().id(), i < list.len());
            self.show(self.params[i].get().id(), i < list.len());
        }
    }
}

/// Applies a panel edit to the model; whether anything changed.
pub fn apply(st: &mut EditorState, prop: Prop) -> bool {
    // Editing what was grown keeps it: an edit's undo step must start from it.
    if !matches!(prop, Prop::Voxel(_)) {
        st.commit_pending();
    }
    match prop {
        Prop::Op(i) => st.edit_selected_brush(|b| {
            b.op = if i == 0 { Op::Add } else { Op::Subtract };
        }),
        Prop::Material(i) => st.edit_selected_brush(|b| b.set_material(i as u16)),
        Prop::Min(a, v) => st.edit_selected_brush(|b| {
            if let Shape::Box { min, .. } = &mut b.shape {
                min[a] = v;
            }
        }),
        Prop::Max(a, v) => st.edit_selected_brush(|b| {
            if let Shape::Box { max, .. } = &mut b.shape {
                max[a] = v;
            }
        }),
        Prop::Pos(a, v) => st.edit_selected_entity(|e: &mut Entity| {
            let mut p = pos(e);
            p[a] = v;
            *e = with_pos(e, p);
        }),
        Prop::Param(i, v) => st.edit_selected_entity(|e| set_param(e, i, v)),
        Prop::NameText(text) => {
            st.sync_draft();
            st.draft.name = text;
            false
        }
        Prop::TagsText(text) => {
            st.sync_draft();
            st.draft.tags = text;
            false
        }
        Prop::ApplyNames => st.apply_draft(),
        Prop::Voxel(prop) => apply_voxel(st, prop),
    }
}
