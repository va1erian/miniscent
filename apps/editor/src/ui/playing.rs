//! Going in and out of Instant Play.

use super::app::Editor;
use crate::studio::Preview;

impl Editor {
    /// Says on the status bar how far the picture has caught up.
    pub(super) fn show_preview(&self, preview: Preview) {
        if self.preview.replace(preview) != preview {
            self.status.get().set_text(3, preview.label());
        }
    }

    /// F5: play the level as it is, as soon as the preview has caught up. The
    /// script's edits go to its file first, since that is what the game reads.
    /// Lighting still being baked arrives during the game.
    pub(super) fn play_command(&mut self) {
        self.state.borrow_mut().commit_pending();
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
        } else if self.studio.playable_now(&doc) {
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
                // First: the main window must hold the focus to take the pointer.
                self.script.suspend();
                self.close_fog_window();
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
        self.script.resume();
        if let Some(m) = message {
            self.state.borrow_mut().message = m.into();
        }
        self.refresh();
    }
}
