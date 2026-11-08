//! The Voxel tool's section of the properties panel: the dab and its settings,
//! the generators with their seed and Commit, and Fill.

use mscent_render::MaterialTable;
use mscent_world::materials::NAMES;
use mscent_world::voxel::CellSize;
use xui::backend::{Result as XuiResult, WidgetId};
use xui::prelude::*;
use xui::widget::GroupBox;

use super::Msg;
use super::faces::{MAX_RADIUS, MIN_RADIUS};
use super::props::Mount;
use super::quiet;
use crate::model::generators::{FieldKind, Kind};
use crate::model::stroke::VoxelOp;
use crate::model::{EditorState, Tool};

/// A change made in the Voxel section.
#[derive(Clone, Debug, PartialEq)]
pub enum VoxelProp {
    Op(usize),
    Radius(f64),
    /// A material id.
    Material(u8),
    Cell(usize),
    /// 0 is no generator: clicks dab.
    Generator(usize),
    Seed(f64),
    Field(usize),
    Wavelength(f64),
    Commit,
    Fill,
}

const CELL_NAMES: [&str; 6] = ["0.125", "0.25", "0.5", "1", "2", "4"];
const CELLS: [CellSize; 6] = [
    CellSize::Eighth,
    CellSize::Quarter,
    CellSize::Half,
    CellSize::One,
    CellSize::Two,
    CellSize::Four,
];
const MAX_SEED: f64 = u32::MAX as f64;

/// Handles to the section's widgets.
#[derive(Default)]
pub struct VoxelPanel {
    pub group: Handle<GroupBox<Msg>>,
    op: Handle<ComboBox<Msg>>,
    radius: Handle<NumberField<Msg>>,
    material: Handle<ComboBox<Msg>>,
    /// Holds `material`, which is made again for a new material table.
    material_host: Handle<xui::widget::Panel<Msg>>,
    /// The material list's rows: an id a voxel can hold, and its name.
    materials: Vec<(u8, String)>,
    cell: Handle<ComboBox<Msg>>,
    generator: Handle<ComboBox<Msg>>,
    seed: Handle<NumberField<Msg>>,
    field: Handle<ComboBox<Msg>>,
    wavelength: Handle<NumberField<Msg>>,
}

fn send(p: VoxelProp) -> Msg {
    Msg::Prop(super::props::Prop::Voxel(p))
}

/// Every material a voxel can hold: the table's ids that fit a byte, or the
/// stock ones without a table.
fn voxel_materials(table: Option<&MaterialTable>) -> Vec<(u8, String)> {
    match table {
        Some(t) => t
            .iter()
            .filter_map(|(id, m)| Some((u8::try_from(id.0).ok()?, m.name.clone())))
            .collect(),
        None => (0u8..).zip(NAMES.map(String::from)).collect(),
    }
}

/// The material list, in a layout of its own to mount in its container.
fn material_list(rows: &[(u8, String)], handle: &Handle<ComboBox<Msg>>) -> Layout<Msg> {
    let ids: Vec<u8> = rows.iter().map(|(id, _)| *id).collect();
    let names: Vec<&str> = rows.iter().map(|(_, n)| n.as_str()).collect();
    let list = combo_box(&names)
        .on_select(move |row| send(VoxelProp::Material(ids.get(row).copied().unwrap_or(0))))
        .bind(handle);
    column().child(list)
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
                panel(material_list(&self.materials, &self.material))
                    .plain()
                    .bind(&self.material_host),
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

    /// Makes the material list again from `table`. After an error the old list
    /// stays, and works as it did.
    pub fn show_materials(
        &mut self,
        table: Option<&MaterialTable>,
        mount: &Mount<'_>,
    ) -> XuiResult<()> {
        let rows = voxel_materials(table);
        let list = Handle::new();
        if let Some(host) = self.material_host.try_get() {
            mount(&host, material_list(&rows, &list))?;
            self.material = list;
        }
        self.materials = rows;
        Ok(())
    }

    pub fn sync(&self, st: &EditorState, show: impl Fn(WidgetId, bool)) {
        show(self.group.get().id(), st.tool == Tool::Voxel);
        let tool = &st.voxel;
        let at = |list: &[VoxelOp]| list.iter().position(|&o| o == tool.op).unwrap_or(0);
        quiet::select(&self.op.get(), at(&VoxelOp::ALL));
        self.radius.get().set_value(tool.radius);
        let material = self.materials.iter().position(|(id, _)| *id == tool.material);
        quiet::select(&self.material.get(), material.unwrap_or(0));
        let cell = CELLS.iter().position(|&c| c == tool.cell).unwrap_or(1);
        quiet::select(&self.cell.get(), cell);
        let generator = tool
            .generator
            .and_then(|k| Kind::ALL.iter().position(|&x| x == k))
            .map_or(0, |i| i + 1);
        quiet::select(&self.generator.get(), generator);
        self.seed.get().set_value(f64::from(tool.seed));
        let field = FieldKind::ALL.iter().position(|&f| f == tool.field);
        quiet::select(&self.field.get(), field.unwrap_or(0));
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
            false
        }
        VoxelProp::Radius(v) => {
            st.voxel.radius = v.clamp(MIN_RADIUS, MAX_RADIUS);
            false
        }
        VoxelProp::Material(id) => {
            st.voxel.material = id;
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
