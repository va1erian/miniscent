//! The Texture Studio workspace: a child of the window's outer stack, so it covers
//! the palette and the side panel. The material list is on the left, the preview
//! and the block page in the middle and the selected block's parameters on the
//! right. All of it works from [`crate::texstudio::TexStudio`], which holds the
//! recipes and talks to the worker; this module only shows that and passes on what
//! is clicked.
//!
//! The preview has one tab per thing it can show. Block shows the selected block,
//! the others one map of the finished material whichever block is selected, so a
//! tab stays chosen while editing.

use std::cell::RefCell;
use std::rc::Rc;

use mscent_world::texinfo::DEFAULT_SCALE;
use xui::Image;
use xui::backend::{Result as XuiResult, WidgetId};
use xui::prelude::*;
use xui::widget::{HasText, Menu};

use super::app::{Msg, cmd};
use crate::materials::SharedMaterials;
use crate::texstudio::{Picture, Row, Tab, TexStudio, Thumbs};

mod brush;
mod commands;
mod dialogs;
mod msg;
mod page;
mod page_geom;
mod page_menu;
mod page_paint;
mod params;
mod preview;
mod reads;

use page::Page;
use page_menu::{MENU_DELETE, MENU_DUPLICATE, page_menu};
use params::{Host, ParamPanel};
use preview::Preview;

pub use msg::{BrushEdit, TexMsg};

/// The preview pane as a scripted run reaches it.
pub type TexPreview = Preview<Msg>;

/// Width of the material column, in dip.
const LEFT_WIDTH: i32 = 190;
/// Width of the parameter column, in dip.
const RIGHT_WIDTH: i32 = 300;
/// Height of the block page, in dip: seven rows, so a stock recipe's chain shows in
/// pieces long enough to read; the rest scrolls.
const PAGE_HEIGHT: i32 = 392;

const TABS: [(Tab, &str); 6] = [
    (Tab::Block, "Block"),
    (Tab::Albedo, "Albedo"),
    (Tab::Height, "Height"),
    (Tab::Normal, "Normal"),
    (Tab::Specular, "Specular"),
    (Tab::Lit, "Lit"),
];

fn tex(msg: TexMsg) -> Msg {
    Msg::Tex(msg)
}

/// The workspace's widgets and its model.
pub struct Workspace {
    pub(super) studio: Rc<RefCell<TexStudio>>,
    host: Handle<Host>,
    list_host: Handle<Host>,
    rows_shown: Vec<Row>,
    /// One per row: a click must not leave a row looking other than it is.
    row_buttons: Vec<Handle<ToggleButton<Msg>>>,
    /// The generation of the material table the list was made from.
    generation_shown: Option<u64>,
    pub(super) preview: Handle<Preview<Msg>>,
    page: Handle<Page>,
    params: ParamPanel,
    tabs: [Handle<ToggleButton<Msg>>; TABS.len()],
    tiles: [Handle<ToggleButton<Msg>>; 2],
    caption: Handle<Label<Msg>>,
    trouble: Handle<Label<Msg>>,
    size_note: [Handle<Label<Msg>>; 2],
    gloss_slider: Handle<Slider<Msg>>,
    gloss_field: Handle<NumberField<Msg>>,
    /// The material and epoch the gloss widgets show.
    gloss_of: Option<(String, u64)>,
    /// The page's menu, which needs the window to overhang the workspace.
    menu: Option<Menu<Msg>>,
    /// The cell its last opening was over.
    menu_cell: Rc<std::cell::Cell<(u16, u16)>>,
    /// The material and block the page was last scrolled to show.
    revealed: RefCell<Option<(String, Option<usize>)>>,
}

fn gloss_msg(v: f64, live: bool) -> Option<Msg> {
    Some(tex(TexMsg::Gloss {
        value: v.round().clamp(0.0, 255.0) as u8,
        live,
    }))
}

fn rows_layout(rows: &[Row], buttons: &[Handle<ToggleButton<Msg>>]) -> Layout<Msg> {
    let mut list = column().gap(2);
    for (row, handle) in rows.iter().zip(buttons) {
        let text = if row.dirty {
            format!("{} *", row.name)
        } else {
            row.name.clone()
        };
        let name = row.name.clone();
        list = list.child(
            toggle_button(text)
                .checked(row.current)
                .on_toggle(move |_| tex(TexMsg::Open(name.clone())))
                .bind(handle),
        );
    }
    column().child(scroll(list).fill(1))
}

/// A material's size in texels, and the world size it repeats at with the default
/// scale.
fn size_notes(width: u32, height: u32) -> (String, String) {
    let world = |side: u32| side as f32 * DEFAULT_SCALE;
    (
        format!("{width} x {height} texels"),
        format!("tile {} x {} units", world(width), world(height)),
    )
}

impl Workspace {
    pub fn new(materials: SharedMaterials) -> Self {
        Self {
            studio: Rc::new(RefCell::new(TexStudio::new(materials))),
            host: Handle::new(),
            list_host: Handle::new(),
            rows_shown: Vec::new(),
            row_buttons: Vec::new(),
            generation_shown: None,
            preview: Handle::new(),
            page: Handle::new(),
            params: ParamPanel::default(),
            tabs: Default::default(),
            tiles: Default::default(),
            caption: Handle::new(),
            trouble: Handle::new(),
            size_note: Default::default(),
            gloss_slider: Handle::new(),
            gloss_field: Handle::new(),
            gloss_of: None,
            menu: None,
            menu_cell: Rc::new(std::cell::Cell::new((0, 0))),
            revealed: RefCell::new(None),
        }
    }

    fn left_column(&self) -> Layout<Msg> {
        let gloss = row().gap(4).children((
            slider(0.0, 255.0)
                .then(|s| {
                    s.on_change(|v| gloss_msg(v, true))
                        .on_commit(|v| gloss_msg(v, false))
                })
                .bind(&self.gloss_slider)
                .fill(1),
            number_field(0.0, 255.0, 5.0)
                .then(|f| f.on_commit(|v| gloss_msg(v, false)))
                .bind(&self.gloss_field)
                .width(70),
        ));
        column()
            .padding(8)
            .gap(6)
            .child(label("Materials").title())
            .child(panel(column()).plain().bind(&self.list_host).fill(1))
            .child(button("New...").on_click(tex(TexMsg::AskNew)))
            .child(label("").bind(&self.size_note[0]))
            .child(label("").bind(&self.size_note[1]))
            .child(label("Gloss"))
            .child(gloss)
            .child(
                button("Apply")
                    .on_click(tex(TexMsg::Apply))
                    .tooltip("Use this material in the running editor; nothing is written to disk"),
            )
            .child(
                button("Save")
                    .primary()
                    .on_click(tex(TexMsg::Save))
                    .tooltip(
                        "Write the recipe and materials.txt, and use it in the running editor",
                    ),
            )
            .child(button("Back to the level").on_click(Msg::Menu(cmd::TEX_CLOSE)))
    }

    fn middle_column(&self) -> Layout<Msg> {
        let mut tabs = row().gap(2);
        for (i, (tab, name)) in TABS.iter().enumerate() {
            let tab = *tab;
            tabs = tabs.child(
                toggle_button(*name)
                    .checked(tab == Tab::Block)
                    .on_toggle(move |_| tex(TexMsg::Tab(tab)))
                    .bind(&self.tabs[i]),
            );
        }
        let tiling = row().gap(2).children((
            toggle_button("1x")
                .checked(true)
                .on_toggle(|_| tex(TexMsg::Tiled(false)))
                .bind(&self.tiles[0]),
            toggle_button("2x tiled")
                .on_toggle(|_| tex(TexMsg::Tiled(true)))
                .bind(&self.tiles[1]),
        ));
        let bar = row().gap(12).children((
            tabs,
            tiling,
            label("").bind(&self.caption).align_y(Align::Center).fill(1),
        ));
        let studio = self.studio.clone();
        column()
            .padding(8)
            .gap(6)
            .child(bar)
            .child(
                build(|ui| Preview::new(ui, |stroking| tex(TexMsg::Paint(stroking))))
                    .bind(&self.preview)
                    .fill(1),
            )
            .child(label("").bind(&self.trouble))
            .child(
                build(move |ui| Page::new(ui, studio))
                    .bind(&self.page)
                    .height(PAGE_HEIGHT),
            )
    }

    /// The whole workspace, in a plain panel that shows and hides as one.
    pub fn layout(&self) -> impl IntoEntry<Msg> + use<> {
        // Room on the right for the scroll bar, so it never covers a field.
        let params = column().padding(8).child(
            panel(params::nothing_selected())
                .plain()
                .bind(&self.params.host),
        );
        let right = column().child(scroll(params).fill(1));
        let content = row().children((
            self.left_column().width(LEFT_WIDTH),
            self.middle_column().fill(1),
            right.width(RIGHT_WIDTH),
        ));
        panel(content).plain().bind(&self.host)
    }

    /// What has to wait until the window exists.
    pub fn attach(&mut self, ui: &Ui<Msg>) {
        self.params.attach(ui);
        self.menu = Some(page_menu(ui, self.menu_cell.clone()));
    }

    pub fn is_open(&self) -> bool {
        self.studio.borrow().is_open()
    }

    pub fn set_visible(&self, visible: bool) {
        if let Some(host) = self.host.try_get() {
            host.set_visible(visible);
        }
    }

    /// Whether `id` takes typed keys, so shortcuts leave it alone.
    pub fn owns(&self, id: WidgetId) -> bool {
        self.params.owns(id) || self.gloss_field.try_get().is_some_and(|f| f.id() == id)
    }

    fn sync_list(&mut self) -> XuiResult<()> {
        let rows = self.studio.borrow().rows();
        if rows != self.rows_shown {
            let buttons: Vec<_> = rows.iter().map(|_| Handle::new()).collect();
            if let Some(host) = self.list_host.try_get() {
                host.set_layout(rows_layout(&rows, &buttons))?;
            }
            self.rows_shown = rows;
            self.row_buttons = buttons;
        }
        for (row, button) in self.rows_shown.iter().zip(&self.row_buttons) {
            if let Some(button) = button.try_get() {
                button.set_checked(row.current);
            }
        }
        Ok(())
    }

    fn sync_gloss(&mut self) {
        let studio = self.studio.borrow();
        let doc = studio.doc();
        let key = doc.map(|d| (d.name().to_string(), d.panel_epoch()));
        if key == self.gloss_of {
            return;
        }
        let gloss = doc.map_or(0, |d| d.recipe().gloss);
        self.gloss_slider.get().set_value(f64::from(gloss));
        self.gloss_field.get().set_value(f64::from(gloss));
        let (texels, tile) = doc.map_or_else(Default::default, |d| {
            let size = d.recipe().size;
            size_notes(size.width(), size.height())
        });
        self.size_note[0].get().set_text(&texels);
        self.size_note[1].get().set_text(&tile);
        self.gloss_of = key;
    }

    /// Makes the widgets that depend on the model show it. None of this is costly
    /// when nothing changed.
    fn sync(&mut self) -> XuiResult<()> {
        self.sync_list()?;
        self.sync_gloss();
        {
            let studio = self.studio.borrow();
            self.params.sync(studio.doc(), studio.brush())?;
            self.preview.get().set_paintable(studio.can_paint());
            for (i, (tab, _)) in TABS.iter().enumerate() {
                self.tabs[i].get().set_checked(*tab == studio.tab());
            }
            self.tiles[0].get().set_checked(!studio.tiled());
            self.tiles[1].get().set_checked(studio.tiled());
            self.preview.get().set_tiled(studio.tiled());
        }
        self.sync_trouble();
        self.follow_selection();
        self.page.get().invalidate();
        Ok(())
    }

    /// Scrolls the page to the selected block when the selection is another one
    /// than it last showed. Left for the next call while the page has no size yet.
    fn follow_selection(&self) {
        let studio = self.studio.borrow();
        let Some(doc) = studio.doc() else {
            return;
        };
        let key = Some((doc.name().to_string(), doc.selected()));
        if *self.revealed.borrow() == key {
            return;
        }
        // Another material starts at the top: the last one's place means nothing.
        let same_material =
            self.revealed.borrow().as_ref().map(|k| k.0.as_str()) == Some(doc.name());
        if !same_material {
            self.page.get().reset_scroll();
        }
        let blocks = &doc.recipe().blocks;
        let block = doc.selected().and_then(|s| blocks.get(s));
        if block.is_none_or(|b| self.page.get().reveal(b, blocks)) {
            *self.revealed.borrow_mut() = key;
        }
    }

    /// Says what is wrong with the page, and repaints it if that changed. Only on a
    /// change: new text means a new layout of the window.
    fn sync_trouble(&self) {
        let (_, why) = self.studio.borrow().trouble();
        let why = why.unwrap_or_default();
        if self.trouble.get().text() != why {
            self.trouble.get().set_text(&why);
            self.page.get().invalidate();
        }
    }

    fn show_picture(&self, picture: Picture) {
        let tab = TABS
            .iter()
            .find(|(t, _)| *t == picture.tab)
            .map_or("", |(_, name)| *name);
        let stage = if picture.full { "" } else { ", draft" };
        let size = format!("{} x {}", picture.width, picture.height);
        let caption = match picture.note {
            Some(note) => format!("{tab}: {note}"),
            None => format!("{tab}, {size}{stage}"),
        };
        if self.caption.get().text() != caption {
            self.caption.get().set_text(&caption);
        }
        let preview = self.preview.get();
        if picture.rgba.is_empty() {
            preview.show_text(picture.note.unwrap_or_default());
        } else {
            preview.set_rgba(picture.width, picture.height, picture.rgba);
        }
    }

    fn show_thumbs(&self, thumbs: Thumbs) {
        let images = thumbs
            .images
            .into_iter()
            .map(|t| t.and_then(|(w, h, rgba)| Image::from_rgba(w, h, rgba).ok()))
            .collect();
        self.page.get().set_thumbs(images);
    }

    /// The table's generation changed: the list follows it.
    fn follow_table(&mut self, table: u64) -> XuiResult<()> {
        if self.generation_shown == Some(table) {
            return Ok(());
        }
        self.generation_shown = Some(table);
        self.sync_list()
    }
}

#[cfg(test)]
mod page_tests;
#[cfg(test)]
mod paint_tests;
#[cfg(test)]
mod tests;
