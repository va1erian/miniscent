//! The Voxel tool's section of the properties panel: the dab and its settings,
//! the generators with their seed and Commit, and Fill.

use mscent_world::materials::NAMES;
use mscent_world::voxel::CellSize;
use xui::backend::WidgetId;
use xui::prelude::*;
use xui::widget::GroupBox;

use super::Msg;
use super::faces::{MAX_RADIUS, MIN_RADIUS};
use crate::model::generators::{FieldKind, Kind};
use crate::model::stroke::VoxelOp;
use crate::model::{EditorState, Tool};

/// A change made in the Voxel section.
#[derive(Clone, Debug, PartialEq)]
pub enum VoxelProp {
    Op(usize),
    Radius(f64),
    Material(usize),
    Cell(usize),
    /// 0 is no generator: clicks dab.
    Generator(usize),
    Seed(f64),
    Field(usize),
    Wavelength(f64),
    Commit,
    Fill,
}

const CELL_NAMES: [&str; 4] = ["0.125", "0.25", "0.5", "1"];
const CELLS: [CellSize; 4] = [
    CellSize::Eighth,
    CellSize::Quarter,
    CellSize::Half,
    CellSize::One,
];
const MAX_SEED: f64 = u32::MAX as f64;

/// Handles to the section's widgets.
#[derive(Default)]
pub struct VoxelPanel {
    pub group: Handle<GroupBox<Msg>>,
    op: Handle<ComboBox<Msg>>,
    radius: Handle<NumberField<Msg>>,
    material: Handle<ComboBox<Msg>>,
    cell: Handle<ComboBox<Msg>>,
    generator: Handle<ComboBox<Msg>>,
    seed: Handle<NumberField<Msg>>,
    field: Handle<ComboBox<Msg>>,
    wavelength: Handle<NumberField<Msg>>,
}

fn send(p: VoxelProp) -> Msg {
    Msg::Prop(super::props::Prop::Voxel(p))
}

impl VoxelPanel {
    /// Whether `id` is one of the section's fields or lists.
    pub fn owns(&self, id: WidgetId) -> bool {
        self.radius.get().id() == id
            || self.seed.get().id() == id
            || self.wavelength.get().id() == id
            || [
                &self.op,
                &self.material,
                &self.cell,
                &self.generator,
                &self.field,
            ]
            .iter()
            .any(|h| h.get().id() == id)
    }

    pub fn layout(&self) -> Layout<Msg> {
        let mut names = vec!["None (dab)"];
        names.extend(Kind::NAMES);
        let dabs = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Dab"))
            .child(
                combo_box(&VoxelOp::NAMES)
                    .on_select(|i| send(VoxelProp::Op(i)))
                    .bind(&self.op),
            )
            .child(label("Radius"))
            .child(
                number_field(MIN_RADIUS, MAX_RADIUS, 0.25)
                    .then(|f| f.on_commit(|v| Some(send(VoxelProp::Radius(v)))))
                    .bind(&self.radius),
            )
            .child(label("Material"))
            .child(
                combo_box(&NAMES)
                    .on_select(|i| send(VoxelProp::Material(i)))
                    .bind(&self.material),
            )
            .child(label("New cells"))
            .child(
                combo_box(&CELL_NAMES)
                    .on_select(|i| send(VoxelProp::Cell(i)))
                    .bind(&self.cell),
            );
        let growth = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Generate"))
            .child(
                combo_box(&names)
                    .on_select(|i| send(VoxelProp::Generator(i)))
                    .bind(&self.generator),
            )
            .child(label("Seed"))
            .child(
                number_field(0.0, MAX_SEED, 1.0)
                    .then(|f| f.on_commit(|v| Some(send(VoxelProp::Seed(v)))))
                    .bind(&self.seed),
            );
        let growth = column()
            .gap(6)
            .children((growth, button("Commit").on_click(send(VoxelProp::Commit))));
        let fill = grid([Track::Auto, Track::Fill(1)])
            .gap(6)
            .child(label("Fill with"))
            .child(
                combo_box(&FieldKind::NAMES)
                    .on_select(|i| send(VoxelProp::Field(i)))
                    .bind(&self.field),
            )
            .child(label("Passage"))
            .child(
                number_field(2.0, 64.0, 1.0)
                    .then(|f| f.on_commit(|v| Some(send(VoxelProp::Wavelength(v)))))
                    .bind(&self.wavelength),
            );
        let fill = column().gap(6).children((
            fill,
            button("Fill selected volume").on_click(send(VoxelProp::Fill)),
        ));
        column().gap(8).children((dabs, growth, fill))
    }

    pub fn sync(&self, st: &EditorState, show: impl Fn(WidgetId, bool)) {
        show(self.group.get().id(), st.tool == Tool::Voxel);
        let tool = &st.voxel;
        let at = |list: &[VoxelOp]| list.iter().position(|&o| o == tool.op).unwrap_or(0);
        self.op.get().select(at(&VoxelOp::ALL));
        self.radius.get().set_value(tool.radius);
        self.material
            .get()
            .select(usize::from(tool.material).min(NAMES.len() - 1));
        self.cell
            .get()
            .select(CELLS.iter().position(|&c| c == tool.cell).unwrap_or(1));
        let generator = tool
            .generator
            .and_then(|k| Kind::ALL.iter().position(|&x| x == k))
            .map_or(0, |i| i + 1);
        self.generator.get().select(generator);
        self.seed.get().set_value(f64::from(tool.seed));
        let field = FieldKind::ALL.iter().position(|&f| f == tool.field);
        self.field.get().select(field.unwrap_or(0));
        self.wavelength.get().set_value(tool.wavelength);
    }
}

/// Applies a change of the section; whether the document changed.
pub fn apply(st: &mut EditorState, prop: VoxelProp) -> bool {
    match prop {
        VoxelProp::Op(i) => {
            // A new dab keeps what was grown, and starts no path from an old one.
            st.commit_pending();
            st.voxel.path = None;
            st.voxel.op = VoxelOp::ALL[i.min(VoxelOp::ALL.len() - 1)];
            st.voxel.generator = None;
            false
        }
        VoxelProp::Radius(v) => {
            st.voxel.radius = v.clamp(MIN_RADIUS, MAX_RADIUS);
            false
        }
        VoxelProp::Material(i) => {
            st.voxel.material = u8::try_from(i).unwrap_or(0);
            false
        }
        VoxelProp::Cell(i) => {
            st.voxel.cell = CELLS[i.min(CELLS.len() - 1)];
            false
        }
        VoxelProp::Generator(i) => {
            st.voxel.generator = i.checked_sub(1).and_then(|k| Kind::ALL.get(k)).copied();
            false
        }
        VoxelProp::Seed(v) => {
            let seed = v.clamp(0.0, MAX_SEED) as u32;
            st.voxel.seed = seed;
            // A held result is rolled again with the new seed.
            st.reroll_to(seed)
        }
        VoxelProp::Field(i) => {
            st.voxel.field = FieldKind::ALL[i.min(FieldKind::ALL.len() - 1)];
            false
        }
        VoxelProp::Wavelength(v) => {
            st.voxel.wavelength = if v.is_finite() {
                v.clamp(2.0, 64.0)
            } else {
                2.0
            };
            false
        }
        VoxelProp::Commit => {
            if !st.commit_pending() {
                st.message = "Nothing to commit: click with a generator chosen".into();
            }
            true
        }
        VoxelProp::Fill => st.fill_selected(),
    }
}
