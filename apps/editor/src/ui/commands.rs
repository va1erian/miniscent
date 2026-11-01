//! Menu commands: files, build, play and the edit operations.

use std::path::{Path, PathBuf};

use xui::widget::MenuId;

use super::app::{Editor, cmd};
use crate::files;
use crate::model::Document;
use crate::studio::Mode;

impl Editor {
    pub(super) fn menu_command(&mut self, id: MenuId) {
        if self.studio.mode == Mode::Play {
            return;
        }
        match id {
            cmd::NEW => {
                self.install(Document::new(), None, "New level");
            }
            cmd::SHOWCASE => {
                let doc = Document::from_level(&mscent_world::showcase_level());
                self.install(doc, None, "Loaded the showcase level");
            }
            cmd::OPEN => {
                if let Some(dir) = files::recent().and_then(|p| p.parent().map(Path::to_path_buf)) {
                    self.open_dialog.set_initial_dir(dir);
                }
                self.open_dialog.open();
            }
            cmd::SAVE => match self.path.clone() {
                Some(p) => self.save_path(p),
                None => self.save_dialog.open(),
            },
            cmd::SAVE_AS => self.save_dialog.open(),
            cmd::BUILD => self.start_build(false),
            cmd::PLAY => self.play_command(),
            cmd::RESET_CAMERA => {
                self.studio.reset_camera(&self.state.borrow().doc);
            }
            cmd::FIT => self.refit(),
            _ => self.edit_command(id),
        }
    }

    fn edit_command(&mut self, id: MenuId) {
        let mut st = self.state.borrow_mut();
        match id {
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
            _ => {}
        }
    }

    /// Replaces the document (New, Open, showcase).
    fn install(&mut self, doc: Document, path: Option<PathBuf>, message: &str) {
        self.studio.reset(&doc);
        self.saved = doc.clone();
        self.path = path;
        {
            let mut st = self.state.borrow_mut();
            st.load(doc);
            st.message = message.into();
        }
        self.refit();
        self.show_prompt();
    }

    /// Reads a level file; on failure the open document stays as it is and
    /// the status bar says why.
    pub(super) fn open_path(&mut self, path: &Path) {
        match files::load(path) {
            Ok(doc) => {
                files::remember(path);
                let msg = format!(
                    "Opened {} ({} brushes, {} entities)",
                    path.display(),
                    doc.brushes().len(),
                    doc.entities().len()
                );
                self.install(doc, Some(path.to_path_buf()), &msg);
            }
            Err(e) => {
                self.state.borrow_mut().message = e;
            }
        }
    }

    pub(super) fn save_path(&mut self, path: PathBuf) {
        let doc = self.state.borrow().doc.clone();
        let message = match files::save(&doc, &path) {
            Ok(msg) => {
                files::remember(&path);
                self.saved = doc;
                self.path = Some(path);
                msg
            }
            Err(e) => e,
        };
        self.state.borrow_mut().message = message;
    }

    /// Starts a build on the worker thread (F7); `then_play` is F5's path.
    pub(super) fn start_build(&mut self, then_play: bool) {
        let doc = self.state.borrow().doc.clone();
        match self.studio.start_build(&doc, then_play) {
            Ok(()) => {
                self.push_log(&["--- build started ---".into()]);
                self.state.borrow_mut().message = "Building...".into();
            }
            Err(e) => {
                self.push_log(&[format!("ERROR: {e}")]);
                self.state.borrow_mut().message = format!("Build failed: {e}");
            }
        }
    }

    /// F5: play the level, building first when it changed.
    fn play_command(&mut self) {
        if self.studio.building() {
            self.state.borrow_mut().message =
                "Still building; press F5 again when it finishes".into();
            return;
        }
        let fresh = self.studio.has_build() && !self.studio.stale(&self.state.borrow().doc);
        if fresh {
            self.begin_play();
        } else {
            self.start_build(true);
        }
    }
}
