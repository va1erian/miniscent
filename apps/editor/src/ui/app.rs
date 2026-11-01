//! The editor application: menus, tool palette, viewports and the status
//! bar, wired to the shared [`EditorState`].

use std::cell::RefCell;
use std::rc::Rc;

use mscent_world::Op;
use xui::prelude::*;
use xui::widget::MenuId;

use super::props::{Panel, apply};
use super::view3d::View3d;
use super::viewport::{ViewEvent, Viewport};
use crate::model::{Document, EditorState, EntityKind, Tool, View, state::GRIDS};
use crate::stubs;

#[derive(Clone)]
pub enum Msg {
    Menu(MenuId),
    /// A tool palette button, by index into [`TOOLS`].
    Tool(usize),
    NewOp(usize),
    Grid(usize),
    View(ViewEvent),
    Prop(super::props::Prop),
}

/// The palette: label and tool.
const TOOLS: [(&str, Tool); 6] = [
    ("Select / move", Tool::Select),
    ("Box brush", Tool::Box),
    ("Player start", Tool::Place(EntityKind::PlayerStart)),
    ("Light", Tool::Place(EntityKind::Light)),
    ("Enemy", Tool::Place(EntityKind::Enemy)),
    ("Goal", Tool::Place(EntityKind::Goal)),
];

/// Menu command ids.
pub mod cmd {
    use xui::widget::MenuId;
    pub const NEW: MenuId = MenuId::new(1);
    pub const OPEN: MenuId = MenuId::new(2);
    pub const SAVE: MenuId = MenuId::new(3);
    pub const BUILD: MenuId = MenuId::new(4);
    pub const PLAY: MenuId = MenuId::new(5);
    pub const SHOWCASE: MenuId = MenuId::new(6);
    pub const UNDO: MenuId = MenuId::new(10);
    pub const REDO: MenuId = MenuId::new(11);
    pub const DELETE: MenuId = MenuId::new(12);
    pub const FLIP: MenuId = MenuId::new(13);
    pub const FORWARD: MenuId = MenuId::new(14);
    pub const BACK: MenuId = MenuId::new(15);
    pub const FIT: MenuId = MenuId::new(20);
}

pub struct Editor {
    state: Rc<RefCell<EditorState>>,
    viewports: [Handle<Viewport<Msg>>; 3],
    view3d: Handle<View3d<Msg>>,
    menu: Handle<Menu<Msg>>,
    status: Handle<StatusBar<Msg>>,
    tools: [Handle<ToggleButton<Msg>>; TOOLS.len()],
    op: Handle<RadioGroup<Msg>>,
    panel: Panel,
}

fn menus(m: &mut xui::widget::MenuScope<'_>) {
    m.submenu(MenuId::new(100), "&File", |f| {
        f.item(cmd::NEW, "&New");
        f.item(cmd::OPEN, "&Open...");
        f.item(cmd::SHOWCASE, "Open &showcase level");
        f.item(cmd::SAVE, "&Save");
        f.separator();
        f.item(cmd::BUILD, "&Build");
        f.item(cmd::PLAY, "&Play");
    });
    m.submenu(MenuId::new(101), "&Edit", |e| {
        e.item(cmd::UNDO, "&Undo   Ctrl+Z");
        e.item(cmd::REDO, "&Redo   Ctrl+Y");
        e.separator();
        e.item(cmd::DELETE, "&Delete   Del");
        e.item(cmd::FLIP, "&Flip add/subtract");
        e.item(cmd::FORWARD, "Move &later in order");
        e.item(cmd::BACK, "Move &earlier in order");
    });
    m.submenu(MenuId::new(102), "&View", |v| {
        v.item(cmd::FIT, "&Fit views to level");
    });
}

impl Editor {
    /// Builds the window contents around `doc`.
    pub fn build(ui: &mut Ui<Msg>, doc: Document) -> xui::backend::Result<Editor> {
        let state = Rc::new(RefCell::new(EditorState::new(doc)));
        let app = Editor {
            state: state.clone(),
            viewports: Default::default(),
            view3d: Handle::new(),
            menu: Handle::new(),
            status: Handle::new(),
            tools: Default::default(),
            op: Handle::new(),
            panel: Panel::default(),
        };
        let emit: Rc<dyn Fn(ViewEvent) -> Msg> = Rc::new(Msg::View);
        let pane = |i: usize| {
            let (state, emit) = (state.clone(), emit.clone());
            let view = View::ALL[i];
            build(move |ui| Viewport::new(ui, view, state, emit)).bind(&app.viewports[i])
        };
        let views = column().gap(4).children((
            row()
                .gap(4)
                .children((pane(0).fill(1), pane(1).fill(1)))
                .fill(1),
            row()
                .gap(4)
                .children((
                    pane(2).fill(1),
                    build(View3d::new).bind(&app.view3d).fill(1),
                ))
                .fill(1),
        ));
        let mut tools = column().gap(4);
        for (i, (name, _)) in TOOLS.iter().enumerate() {
            tools = tools.child(
                toggle_button(*name)
                    .checked(i == 0)
                    .on_toggle(move |_| Msg::Tool(i))
                    .bind(&app.tools[i]),
            );
        }
        let grid_names: Vec<String> = GRIDS.iter().map(|g| format!("{g}")).collect();
        let grid_refs: Vec<&str> = grid_names.iter().map(String::as_str).collect();
        let palette = scroll(
            column().padding(8).gap(8).children((
                group("Tools", tools),
                group(
                    "New brush",
                    column().child(
                        radio_group(&["Subtract", "Add"])
                            .selected(0)
                            .on_select(Msg::NewOp)
                            .bind(&app.op),
                    ),
                ),
                group(
                    "Grid size",
                    column().child(combo_box(&grid_refs).on_select(Msg::Grid).then(|c| {
                        c.select(2);
                        c
                    })),
                ),
            )),
        );
        ui.root(
            column().children((
                menu_bar(menus).on_select(Msg::Menu).bind(&app.menu),
                row()
                    .children((
                        palette.width(150),
                        views.fill(1),
                        app.panel.layout().width(250),
                    ))
                    .fill(1),
                status_bar(&["Ready", "", ""]).bind(&app.status),
            )),
        )?;
        app.refresh();
        Ok(app)
    }

    /// Repaints the views, refills the panel and rewrites the status bar.
    fn refresh(&self) {
        for v in &self.viewports {
            v.get().invalidate();
        }
        let st = self.state.borrow();
        self.panel.sync(&st);
        let bar = self.status.get();
        bar.set_text(0, &st.message);
        bar.set_text(
            2,
            &format!(
                "{} brushes  {} entities  grid {}",
                st.doc.brushes().len(),
                st.doc.entities().len(),
                st.grid
            ),
        );
        let menu = self.menu.get();
        menu.set_enabled(cmd::UNDO, st.history.can_undo());
        menu.set_enabled(cmd::REDO, st.history.can_redo());
        let any = st.selection.is_some();
        let brush = st.selected_brush().is_some();
        menu.set_enabled(cmd::DELETE, any);
        for id in [cmd::FLIP, cmd::FORWARD, cmd::BACK] {
            menu.set_enabled(id, brush);
        }
        for (i, (_, tool)) in TOOLS.iter().enumerate() {
            self.tools[i].get().set_checked(*tool == st.tool);
        }
        self.op.get().select(usize::from(st.new_op == Op::Add));
    }

    fn menu(&mut self, id: MenuId) {
        let mut st = self.state.borrow_mut();
        match id {
            cmd::NEW => {
                st.load(Document::new());
                st.message = "New level".into();
                drop(st);
                self.refit();
            }
            cmd::SHOWCASE => {
                st.load(Document::from_level(&mscent_world::showcase_level()));
                st.message = "Loaded the showcase level".into();
                drop(st);
                self.refit();
            }
            cmd::OPEN => st.message = stubs::open(),
            cmd::SAVE => st.message = stubs::save(&st.doc),
            cmd::BUILD => st.message = stubs::build(&st.doc),
            cmd::PLAY => st.message = stubs::play(&st.doc),
            cmd::UNDO => {
                st.undo();
            }
            cmd::REDO => {
                st.redo();
            }
            cmd::DELETE => {
                st.delete_selected();
            }
            cmd::FLIP => {
                st.flip_selected();
            }
            cmd::FORWARD => {
                st.reorder_selected(1);
            }
            cmd::BACK => {
                st.reorder_selected(-1);
            }
            cmd::FIT => {
                drop(st);
                self.refit();
            }
            _ => {}
        }
    }

    fn refit(&self) {
        for v in &self.viewports {
            v.get().refit();
        }
    }

    /// The model, shared with the views.
    #[cfg(test)]
    pub fn state(&self) -> Rc<RefCell<EditorState>> {
        self.state.clone()
    }

    /// The 3D pane, for the Play integration.
    #[allow(dead_code, reason = "used by the Play work item; exercised in tests")]
    pub fn view3d(&self) -> Rc<View3d<Msg>> {
        self.view3d.get()
    }
}

#[cfg(test)]
mod tests;

impl App for Editor {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, _ui: &mut Ui<Msg>) {
        match msg {
            Msg::Menu(id) => self.menu(id),
            Msg::Tool(i) => self.state.borrow_mut().tool = TOOLS[i].1,
            Msg::NewOp(i) => {
                self.state.borrow_mut().new_op = if i == 0 { Op::Subtract } else { Op::Add };
            }
            Msg::Grid(i) => self.state.borrow_mut().grid = GRIDS[i.min(GRIDS.len() - 1)],
            Msg::Prop(p) => {
                apply(&mut self.state.borrow_mut(), p);
            }
            Msg::View(ViewEvent::Hover(view, p)) => {
                let (a, b) = view.axes();
                let n = ["X", "Y", "Z"];
                self.status
                    .get()
                    .set_text(1, &format!("{} {:.2}   {} {:.2}", n[a], p[0], n[b], p[1]));
                return;
            }
            Msg::View(ViewEvent::Changed) => {}
        }
        self.refresh();
    }
}
