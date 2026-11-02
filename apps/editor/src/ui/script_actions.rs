//! What the script pane's messages do, and keeping the pane in step with the
//! model on every tick.

use std::path::Path;

use xui::prelude::Ui;
use xui::widget::MenuId;

use super::app::{Editor, Msg, cmd};
use super::discard_dialog::may_discard;
use super::script_pane::ScriptMsg;
use crate::files;
use crate::model::Tool;
use crate::script::buffer::WidgetEdit;
use crate::script::complete::opens_names;
use crate::script::kit::KIT;
use crate::script::samples::SAMPLES;

/// Script menu ids of the basic entities and the samples, in table order.
pub(super) const KIT_MENU: [MenuId; KIT.len()] = {
    let mut ids = [MenuId::new(110); KIT.len()];
    let mut i = 0;
    while i < KIT.len() {
        ids[i] = MenuId::new(110 + i);
        i += 1;
    }
    ids
};
pub(super) const SAMPLE_MENU: [MenuId; SAMPLES.len()] = {
    let mut ids = [MenuId::new(130); SAMPLES.len()];
    let mut i = 0;
    while i < SAMPLES.len() {
        ids[i] = MenuId::new(130 + i);
        i += 1;
    }
    ids
};

/// Whether `msg` puts another level's script in the buffer, which loses edits.
/// The samples list's first row is its label, and opens nothing.
pub(super) fn replaces_script(msg: &Msg) -> bool {
    match msg {
        Msg::OpenPath(_) => true,
        Msg::Menu(id) => *id == cmd::NEW || *id == cmd::SHOWCASE || SAMPLE_MENU.contains(id),
        Msg::Script(ScriptMsg::Sample(i)) => *i < SAMPLES.len(),
        _ => false,
    }
}

impl Editor {
    pub(super) fn script_message(&mut self, msg: ScriptMsg) {
        match msg {
            ScriptMsg::Edited => self.script_edited(),
            ScriptMsg::Problem(row) => self.go_to_problem(row),
            ScriptMsg::InsertExample => self.insert_example(),
            ScriptMsg::GuideEdited => self.script.restore_guide(),
            ScriptMsg::Reload => self.reload_script(),
            ScriptMsg::Save => self.save_command(),
            ScriptMsg::Play => self.play_command(),
            ScriptMsg::Topic(i) => self.script.show_topic(i),
            ScriptMsg::Float(floats) => self.script.set_floating(floats),
            ScriptMsg::Kit(_) | ScriptMsg::Sample(_) | ScriptMsg::Toggle | ScriptMsg::ShowGuide => {
                self.script_menu(msg)
            }
        }
        self.sync_script();
    }

    fn script_edited(&mut self) {
        let code = self.script.code();
        let text = code.text();
        // Nothing holds a name yet, so the popup would not open by itself.
        if opens_names(&text, code.caret()) {
            code.trigger_completion();
        }
        let mut st = self.state.borrow_mut();
        st.script.edited(text);
        self.panel.show_script_uses(&st);
    }

    /// Whether what is about to replace the script may: there are no edits to
    /// lose, or the user says so.
    pub(super) fn script_may_go(&self, ui: &Ui<Msg>) -> bool {
        // Not held across the dialog, whose loop may want the state.
        let dirty = self.state.borrow().script.dirty();
        !dirty || may_discard(ui)
    }

    /// Esc with a basic entity picked goes back to Select, placing nothing.
    pub(super) fn drop_picked_kit(&mut self) {
        let mut st = self.state.borrow_mut();
        if matches!(st.tool, Tool::Kit(_)) {
            st.set_tool(Tool::Select);
            st.message = "Ready".into();
            drop(st);
            self.refresh();
        }
    }

    fn go_to_problem(&mut self, row: usize) {
        self.script.show_detail(row);
        if let Some(line) = self.script.problem_line(row) {
            let code = self.script.code();
            code.goto(line, 0);
            code.focus();
        }
    }

    pub(super) fn pick_kit(&mut self, i: usize) {
        if let Some(entry) = KIT.get(i) {
            self.state.borrow_mut().arm_kit(entry.kind);
            self.script.set_open(true);
        }
    }

    /// Opens a fresh copy of a sample, to play and take apart. The list's first
    /// row, which is its label, opens nothing.
    pub(super) fn open_sample(&mut self, i: usize) {
        let Some(sample) = SAMPLES.get(i) else {
            return;
        };
        match sample.install(&files::samples_base()) {
            Ok(path) => {
                self.open_path(&path);
                let mut st = self.state.borrow_mut();
                // Otherwise the message says why it did not open.
                if st.doc.level_path() == Some(path.as_path()) {
                    st.message = format!(
                        "{}. F5 plays it. This copy is yours: opening the sample again starts over",
                        sample.title
                    );
                }
            }
            Err(e) => self.state.borrow_mut().message = e,
        }
        self.script.reset_sample_list();
    }

    fn insert_example(&mut self) {
        let Some(example) = self.script.example_here() else {
            self.state.borrow_mut().message = "This page has no examples".into();
            return;
        };
        self.script.show_script();
        let editor = self.script.code();
        editor.insert_text(&example);
        self.state.borrow_mut().script.edited(editor.text());
    }

    fn reload_script(&mut self) {
        let mut st = self.state.borrow_mut();
        let path = st.doc.level_path().map(|p| p.with_extension("st"));
        st.script.reload(path.as_deref());
        st.message = "Took the script from the file".into();
    }

    pub(super) fn toggle_script(&mut self) {
        let open = !self.script.is_open();
        self.script.set_open(open);
        if open {
            self.script.show_script();
        } else {
            self.pane3d().focus();
        }
    }

    /// Writes the script beside `level`, saying how it went. Whether it is safe to
    /// go on: a script that cannot be written must not pass for saved.
    pub(super) fn save_script(&mut self, level: &Path) -> Result<bool, String> {
        self.state
            .borrow_mut()
            .script
            .save(&level.with_extension("st"))
    }

    /// One tick of the pane: the file, the widget, the checker.
    pub(super) fn sync_script(&mut self) {
        let edits = {
            let mut st = self.state.borrow_mut();
            st.poll_script();
            st.script.take_widget_edits()
        };
        let code = self.script.code();
        for edit in edits {
            match edit {
                WidgetEdit::Set(text) => code.set_text(&text),
                WidgetEdit::Append(added) => {
                    let end = code.text().chars().count();
                    code.replace(end, end, &added);
                    // A key may have told the model the text without this yet.
                    self.state.borrow_mut().script.edited(code.text());
                }
            }
        }
        let line = self.state.borrow_mut().show_script_line.take();
        if let Some(line) = line {
            self.script.set_open(true);
            code.goto(line, 0);
        }
        let st = self.state.borrow();
        self.script.tick(&st);
        self.script.show_state(&st.script, &st);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_way_to_another_level_counts_and_nothing_else_does() {
        for msg in [
            Msg::Menu(cmd::NEW),
            Msg::Menu(cmd::SHOWCASE),
            Msg::Menu(SAMPLE_MENU[0]),
            Msg::OpenPath("x.msl".into()),
            Msg::Script(ScriptMsg::Sample(0)),
        ] {
            assert!(replaces_script(&msg));
        }
        for msg in [
            Msg::Menu(cmd::SAVE),
            Msg::Menu(cmd::OPEN),
            Msg::Menu(KIT_MENU[0]),
            Msg::Script(ScriptMsg::Sample(usize::MAX)),
            Msg::Script(ScriptMsg::Edited),
        ] {
            assert!(!replaces_script(&msg));
        }
    }
}
