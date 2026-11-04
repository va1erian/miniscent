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

    /// Hands Play the pane's own window once the backend has made it, and
    /// gives the window up if Play could not show itself there.
    pub(super) fn follow_surface(&mut self) {
        let play = self.play.get();
        // Input the render thread held its inbox against goes now.
        play.flush_forward();
        if self.studio.take_surface_lost() {
            self.studio.set_surface(None);
            play.set_native(false);
        } else if !self.studio.has_surface()
            && let Some(window) = play.native()
        {
            self.studio.set_surface(Some(window));
        }
    }

    /// Opens the Instant Play layer over the whole work area.
    pub(super) fn begin_play(&mut self) {
        match self.studio.enter_play() {
            Ok(()) => {
                // Play covers the work area, and with the workspace up behind it
                // its shortcuts and its lit tab would go on.
                if self.tex.is_open() {
                    self.tex_close();
                }
                // First: the main window must hold the focus to take the pointer.
                self.script.suspend();
                self.close_fog_window();
                let play = self.play.get();
                play.set_visible(true);
                // Out from under Play too: xui repaints every node a frame's
                // damage touches, covered or not, so the editor beneath would
                // be painted again for each picture (56 ms at 1080p).
                self.work_host.get().set_visible(false);
                // Its own window, which the GPU shows Play in as the game does.
                if crate::studio::gpu_view::wanted() {
                    play.set_native(true);
                }
                self.pane3d().set_grab(false);
                play.set_grab(true);
                play.focus();
                // Focus changes from the editor are not the player's input.
                play.take_events();
                play.set_forward(self.studio.input_sender());
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
        self.play.get().set_forward(None);
        self.studio.leave_play();
        self.studio.set_surface(None);
        self.play.get().set_native(false);
        self.play.get().set_grab(false);
        self.play.get().set_visible(false);
        self.work_host.get().set_visible(true);
        self.pane3d().focus();
        self.script.resume();
        if let Some(m) = message {
            self.state.borrow_mut().message = m.into();
        }
        self.refresh();
    }
}
