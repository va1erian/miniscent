//! The frame tick and going in and out of Instant Play.

use super::app::Editor;
use super::script_pane::ScriptMsg;
use crate::studio::{Mode, Preview};

impl Editor {
    /// One UI tick: forwards the active pane's input to the render thread
    /// and shows the newest frame it finished. Nothing here waits.
    pub(super) fn advance(&mut self) {
        // Esc or focus loss ended the pointer lock: the cursor is back, so
        // Play ends rather than running with a free cursor.
        if self.studio.mode == Mode::Play && self.play.get().take_lock_lost() {
            self.end_play(Some("Back in the editor"));
        }
        let pane = if self.studio.mode == Mode::Play {
            self.play.get()
        } else {
            self.pane3d()
        };
        let events = pane.take_events();
        let clicks = pane.take_clicks();
        let size = pane.size();
        let state = self.state.clone();
        let tick = self.studio.tick(&events, size, &state.borrow().doc);
        self.follow_materials();
        self.push_log(&tick.log);
        if let Some(s) = &tick.status {
            self.state.borrow_mut().message = s.clone();
            self.set_status(s);
        }
        if let Some(text) = &tick.blank {
            self.pane3d().show_text(text);
        }
        self.show_preview(self.studio.preview(&state.borrow().doc));
        if tick.play_now {
            self.begin_play();
        }
        if tick.left_play {
            self.end_play(None);
        }
        if let Some(f) = tick.frame {
            let target = match f.mode {
                Mode::Play => self.play.get(),
                Mode::Edit => self.pane3d(),
            };
            target.set_rgba(f.w, f.h, f.rgba);
        }
        if self.studio.mode == Mode::Edit {
            self.sync_script();
            if !clicks.is_empty() {
                for click in clicks {
                    self.pick_click(click);
                }
                self.refresh();
            }
        }
    }

    /// Says on the status bar how far the picture has caught up.
    fn show_preview(&self, preview: Preview) {
        if self.preview.replace(preview) != preview {
            self.status.get().set_text(3, preview.label());
        }
    }

    /// F5: play the level as it is, as soon as the preview has caught up. The
    /// script's edits go to its file first, since that is what the game reads.
    /// Lighting still being baked arrives during the game.
    pub(super) fn play_command(&mut self) {
        let doc = self.state.borrow().doc.clone();
        let has_script = !self.state.borrow().script.text().is_empty();
        let mut new_script = false;
        if self.state.borrow().script.conflict() {
            self.state.borrow_mut().message =
                "The script changed on disk: Save overwrites it, Reload takes it".into();
            return;
        }
        match doc.level_path() {
            Some(level) => match self.save_script(level) {
                Ok(wrote) => new_script = wrote,
                Err(e) => {
                    self.state.borrow_mut().message = format!("Cannot play: {e}");
                    return;
                }
            },
            None if has_script => {
                self.state.borrow_mut().message =
                    "Save the level first: its script lives in a file beside it".into();
                self.save_dialog.open();
                return;
            }
            None => {}
        }
        if new_script {
            self.studio.script_changed();
        } else if self.studio.is_current(&doc) {
            self.begin_play();
            return;
        }
        let message = match self.studio.play_when_ready(&doc) {
            Ok(()) => "Preparing the level...".to_string(),
            Err(e) => e,
        };
        self.state.borrow_mut().message = message;
    }

    /// Opens the Instant Play layer over the whole work area.
    pub(super) fn begin_play(&mut self) {
        match self.studio.enter_play() {
            Ok(()) => {
                let play = self.play.get();
                play.set_visible(true);
                self.pane3d().set_grab(false);
                play.set_grab(true);
                play.focus();
                // Focus changes from the editor are not the player's input.
                play.take_events();
                let text = "Playing: Esc = back, WASD (ZQSD on AZERTY) move, Space fire, Q/E roll";
                self.state.borrow_mut().message = text.into();
                self.set_status(text);
            }
            Err(e) => {
                self.state.borrow_mut().message = e.clone();
                self.set_status(&e);
            }
        }
    }

    /// Back to the editor; the document is exactly as it was.
    pub(super) fn end_play(&mut self, message: Option<&str>) {
        self.studio.leave_play();
        self.play.get().set_grab(false);
        self.play.get().set_visible(false);
        self.pane3d().focus();
        if let Some(m) = message {
            self.state.borrow_mut().message = m.into();
        }
        self.refresh();
    }

    /// The Script pane's Save is File > Save: a level and its script go together.
    pub(super) fn save_command(&mut self) {
        match self.path.clone() {
            Some(p) => self.save_path(p),
            None => self.save_dialog.open(),
        }
    }

    /// Script > Show script pane and the guide, from the menu.
    pub(super) fn script_menu(&mut self, msg: ScriptMsg) {
        match msg {
            ScriptMsg::Kit(i) => self.pick_kit(i),
            ScriptMsg::Sample(i) => self.open_sample(i),
            ScriptMsg::Toggle => self.toggle_script(),
            ScriptMsg::ShowGuide => {
                self.script.set_open(true);
                self.script.show_guide();
            }
            _ => {}
        }
    }
}
