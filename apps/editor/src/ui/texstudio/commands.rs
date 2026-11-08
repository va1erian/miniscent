//! What the workspace's messages and menu commands do.

use mscent_texgen::ops::{self, Value};
use xui::prelude::*;
use xui::widget::{HasText, Placeable};

use super::dialogs;
use super::{BrushEdit, MENU_DELETE, MENU_DUPLICATE, TexMsg};
use crate::texstudio::SaveStart;
use crate::ui::app::{Editor, Msg, cmd};

impl Editor {
    /// Opens the workspace on the first material, if nothing is open yet.
    pub(in crate::ui) fn tex_open(&mut self) {
        // An F5 still waiting for the level would start Play under the workspace.
        self.studio.cancel_play_when_ready();
        self.tex.studio.borrow_mut().set_open(true);
        self.work_host.get().set_visible(false);
        self.tex.set_visible(true);
        let first = {
            let studio = self.tex.studio.borrow();
            let open = studio.doc().is_some();
            let rows = studio.rows();
            rows.first().filter(|_| !open).map(|r| r.name.clone())
        };
        if let Some(name) = first {
            self.tex.studio.borrow_mut().open_material(&name);
        }
        self.tex.page.get().focus();
        self.tex_refresh();
    }

    /// Back to the level, which was never touched. The recipes stay open.
    pub(in crate::ui) fn tex_close(&mut self) {
        self.tex.studio.borrow_mut().set_open(false);
        self.tex.set_visible(false);
        self.work_host.get().set_visible(true);
        self.pane3d().focus();
        self.state.borrow_mut().message = "Back in the level".to_string();
        self.refresh();
    }

    /// Shows the model in the workspace's widgets, and its message if it has one.
    fn tex_refresh(&mut self) {
        if let Err(e) = self.tex.sync() {
            self.push_log(&[format!("ERROR: texture studio panels: {e:?}")]);
        }
        self.sync_tex_menu();
    }

    /// Commands that act on the level are off while the workspace is up, and undo
    /// and redo say what the recipe's history can do.
    pub(in crate::ui) fn sync_tex_menu(&self) {
        let open = self.tex.is_open();
        let menu = self.menu.get();
        // Nothing else sets these, so closing has to put them back.
        for id in [
            cmd::NEW,
            cmd::OPEN,
            cmd::SHOWCASE,
            cmd::SAVE,
            cmd::SAVE_AS,
            cmd::PLAY,
            cmd::MAKE_MOVER,
            cmd::FIT,
            cmd::RESET_CAMERA,
            cmd::LAYOUT_CLASSIC,
            cmd::LAYOUT_QUAD,
            cmd::LEVEL_SETTINGS,
            cmd::TEX_OPEN,
        ] {
            menu.set_enabled(id, !open);
        }
        // `refresh` sets these from the level's selection, just before this.
        for id in [
            cmd::FLIP,
            cmd::FORWARD,
            cmd::BACK,
            cmd::CARVE,
            cmd::SLICE,
            cmd::SLICE_SIDE,
            cmd::EXTRUDE,
            cmd::EXTRUDE_IN,
        ] {
            if open {
                menu.set_enabled(id, false);
            }
        }
        for id in [cmd::TEX_CLOSE, cmd::TEX_NEW, cmd::TEX_APPLY, cmd::TEX_SAVE] {
            menu.set_enabled(id, open);
        }
        if !open {
            return;
        }
        let studio = self.tex.studio.borrow();
        let doc = studio.doc();
        menu.set_enabled(cmd::UNDO, doc.is_some_and(|d| d.can_undo()));
        menu.set_enabled(cmd::REDO, doc.is_some_and(|d| d.can_redo()));
        menu.set_enabled(cmd::DELETE, doc.is_some_and(|d| d.selected().is_some()));
    }

    /// One UI tick for the workspace. Nothing here waits.
    pub(in crate::ui) fn tex_advance(&mut self) {
        if !self.tex.studio.borrow().needs_ticks() {
            return;
        }
        let tick = self.tex.studio.borrow_mut().tick();
        if tick.cleared {
            self.tex.preview.get().show_text("Working...");
            self.tex.working.set(true);
            self.tex.page.get().set_thumbs(Vec::new());
            self.tex.caption.get().set_text("");
        }
        let changed = tick.message.is_some() || tick.cleared;
        if let Some(picture) = tick.picture {
            self.tex.working.set(false);
            self.tex.show_picture(picture);
        } else if self.tex.working.get() && self.tex.studio.borrow().settled() {
            // No picture will come: the page does not compile, or its first draft
            // failed. Either way the reason is in the error label, not here.
            self.tex.working.set(false);
            self.tex.preview.get().show_text("");
        }
        if let Some(thumbs) = tick.thumbs {
            self.tex.show_thumbs(thumbs);
        }
        if let Some(message) = tick.message {
            self.state.borrow_mut().message = message.clone();
            self.set_status(&message);
        }
        let generation = self.studio.materials().generation();
        if let Err(e) = self.tex.follow_table(generation) {
            self.push_log(&[format!("ERROR: material list: {e:?}")]);
        }
        if changed {
            self.tex_refresh();
        } else {
            self.tex.sync_trouble();
            self.tex.follow_selection();
        }
    }

    /// A shortcut or menu command while the workspace is up. Those that are not the
    /// workspace's do nothing: they never reach the level.
    pub(in crate::ui) fn tex_menu(&mut self, id: MenuId, ui: &Ui<Msg>) {
        match id {
            cmd::UNDO => {
                self.tex.studio.borrow_mut().undo();
            }
            cmd::REDO => {
                self.tex.studio.borrow_mut().redo();
            }
            cmd::DELETE => self.tex_edit(TexMsg::Delete),
            cmd::SAVE | cmd::TEX_SAVE => self.tex_save(),
            cmd::TEX_APPLY => self.tex_edit(TexMsg::Apply),
            cmd::TEX_NEW => self.tex_ask_new(ui),
            cmd::TEX_CLOSE => {
                self.tex_close();
                return;
            }
            cmd::PLAY => {
                self.set_status("Play is for the level: Esc goes back to it, then F5");
                return;
            }
            _ => {}
        }
        self.tex_refresh();
    }

    fn tex_ask_new(&mut self, ui: &Ui<Msg>) {
        self.tex.new_form.open();
        ui.invalidate_layout();
    }

    /// Makes the material the form describes. A name that cannot be used stays in
    /// the form, with the reason beside it, so what was typed is not lost.
    fn tex_new_submit(&mut self, ui: &Ui<Msg>) {
        let Some((name, side)) = self.tex.new_form.answer() else {
            return;
        };
        let created = self.tex.studio.borrow_mut().create(&name, side);
        match created {
            Ok(()) => self.tex_new_close(ui),
            // The whole reason goes where long messages fit.
            Err(why) => {
                self.tex.new_form.say("That name cannot be used");
                self.tex.studio.borrow_mut().say(why);
            }
        }
    }

    fn tex_new_close(&mut self, ui: &Ui<Msg>) {
        self.tex.new_form.close();
        ui.invalidate_layout();
    }

    /// Saves, unless that needs the question first: its answer saves or not.
    fn tex_save(&mut self) {
        let start = self.tex.studio.borrow_mut().save();
        if let SaveStart::NeedsConsent(name) = start
            && let Some(question) = &self.tex.overwrite
        {
            question.set_message(&dialogs::overwrite_question(&name));
            question.open();
            self.tex.overwriting = Some(name);
        }
    }

    fn tex_confirm_overwrite(&mut self) {
        if let Some(name) = self.tex.overwriting.take() {
            let mut studio = self.tex.studio.borrow_mut();
            studio.consent(&name);
            studio.save();
        }
    }

    /// Takes a key that the window's shortcuts would otherwise act on, for the
    /// question or the form on top: Esc says no and Enter yes. Whether it did.
    pub(in crate::ui) fn tex_answer_key(&mut self, enter: bool, ui: &Ui<Msg>) -> bool {
        if let Some(question) = self.tex.overwrite.as_ref().filter(|q| q.is_open()) {
            question.close();
            match enter {
                true => self.tex_confirm_overwrite(),
                false => self.tex.overwriting = None,
            }
        } else if self.tex.new_form.is_open() {
            match enter {
                true => self.tex_new_submit(ui),
                false => self.tex_new_close(ui),
            }
        } else {
            return false;
        }
        self.tex_refresh();
        true
    }

    /// A message from the workspace.
    pub(in crate::ui) fn tex_message(&mut self, msg: TexMsg, ui: &Ui<Msg>) {
        match msg {
            TexMsg::AskNew => self.tex_ask_new(ui),
            TexMsg::NewSubmit => self.tex_new_submit(ui),
            TexMsg::NewCancel => self.tex_new_close(ui),
            TexMsg::Save => self.tex_save(),
            TexMsg::ConfirmOverwrite => self.tex_confirm_overwrite(),
            TexMsg::CancelOverwrite => self.tex.overwriting = None,
            other => self.tex_edit(other),
        }
        self.tex_refresh();
    }

    fn tex_page_menu(&self, x: i32, y: i32, cell: (u16, u16), block: Option<usize>) {
        let (Some(menu), Some(host)) = (&self.tex.menu, self.tex.host.try_get()) else {
            return;
        };
        self.tex.menu_cell.set(cell);
        if block.is_some() {
            self.tex.studio.borrow_mut().edit(|d| {
                d.select(block);
                Ok(())
            });
        }
        menu.set_enabled(MENU_DELETE, block.is_some());
        menu.set_enabled(MENU_DUPLICATE, block.is_some());
        // Bounds are relative to the parent: the workspace's panel is at the
        // window's level, and the page is directly in it.
        let ui = host.ui();
        let page = Placeable::id(&*self.tex.page.get());
        let (outer, inner) = (ui.bounds(host.id()), ui.bounds(page));
        menu.show_context(outer.left + inner.left + x, outer.top + inner.top + y);
    }

    fn tex_edit(&mut self, msg: TexMsg) {
        if let TexMsg::PageMenu { x, y, cell, block } = msg {
            return self.tex_page_menu(x, y, cell, block);
        }
        if let TexMsg::Param { index, value, .. } = &msg {
            self.tex.params.show_value(*index, value);
        }
        if let TexMsg::Brush(edit) = &msg {
            self.tex.params.show_brush(*edit);
        }
        if let TexMsg::Gloss { value, .. } = &msg {
            self.tex.gloss_slider.get().set_value(f64::from(*value));
            self.tex.gloss_field.get().set_value(f64::from(*value));
        }
        let mut studio = self.tex.studio.borrow_mut();
        match msg {
            TexMsg::Open(name) => studio.open_material(&name),
            TexMsg::Create { name, side } => {
                if let Err(why) = studio.create(&name, side) {
                    studio.say(why);
                }
            }
            TexMsg::Select(block) => {
                studio.edit(|d| {
                    d.select(block);
                    Ok(())
                });
            }
            TexMsg::Move { block, x, y } => {
                studio.edit(|d| d.move_block(block, x, y));
            }
            TexMsg::Resize { block, w } => {
                studio.edit(|d| d.resize(block, w));
            }
            TexMsg::Add { op, cell } => {
                if let Some(spec) = ops::all().nth(op) {
                    studio.edit(|d| d.add(spec, cell.0, cell.1));
                }
            }
            TexMsg::Delete => {
                studio.edit(|d| match d.selected() {
                    Some(block) => d.delete(block),
                    None => Ok(()),
                });
            }
            TexMsg::Duplicate => {
                studio.edit(|d| match d.selected() {
                    Some(block) => d.duplicate(block).map(|_| ()),
                    None => Ok(()),
                });
            }
            TexMsg::Param {
                block,
                index,
                value,
                live,
            } => {
                studio.edit(|d| d.set_param(block, index, value, live));
                studio.note_drag(live);
            }
            TexMsg::Channel {
                block,
                index,
                channel,
                value,
            } => {
                studio.edit(|d| {
                    let values = d.recipe().blocks.get(block).map(|b| b.params.values());
                    let current = values.and_then(|v| v.get(index));
                    let Some(Value::Color(mut colour)) = current.cloned() else {
                        return Ok(());
                    };
                    if let Some(slot) = colour.get_mut(channel) {
                        *slot = value;
                    }
                    d.set_param(block, index, Value::Color(colour), false)
                });
                studio.note_drag(false);
            }
            TexMsg::ClearStrokes { block, index } => {
                studio.edit(|d| d.clear_strokes(block, index));
            }
            TexMsg::Paint(stroking) => studio.paint(stroking),
            TexMsg::Brush(edit) => {
                let mut brush = studio.brush();
                match edit {
                    BrushEdit::Radius(v) => brush.radius = v,
                    BrushEdit::Hardness(v) => brush.hardness = v,
                    BrushEdit::Color(c) => brush.color = c,
                }
                studio.set_brush(brush);
            }
            TexMsg::Gloss { value, live } => {
                studio.edit(|d| {
                    d.set_gloss(value, live);
                    Ok(())
                });
                studio.note_drag(live);
            }
            TexMsg::Tab(tab) => studio.set_tab(tab),
            TexMsg::Tiled(tiled) => studio.set_tiled(tiled),
            TexMsg::Apply => {
                studio.apply();
            }
            TexMsg::AskNew
            | TexMsg::NewSubmit
            | TexMsg::NewCancel
            | TexMsg::Save
            | TexMsg::ConfirmOverwrite
            | TexMsg::CancelOverwrite
            | TexMsg::PageMenu { .. } => {}
        }
    }
}
