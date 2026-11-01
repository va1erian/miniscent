//! Menu commands: files, play and the edit operations.

use std::path::{Path, PathBuf};

use xui::widget::MenuId;

use super::app::{Editor, cmd};
use crate::files;
use crate::model::Document;
use crate::studio::Mode;

/// The menu bar.
pub(super) fn menus(m: &mut xui::widget::MenuScope<'_>) {
    m.submenu(MenuId::new(100), "&File", |f| {
        f.item(cmd::NEW, "&New   Ctrl+N");
        f.item(cmd::OPEN, "&Open...   Ctrl+O");
        f.item(cmd::SHOWCASE, "Open &showcase level");
        f.item(cmd::SAVE, "&Save   Ctrl+S");
        f.item(cmd::SAVE_AS, "Save &as...");
    });
    m.submenu(MenuId::new(101), "&Edit", |e| {
        e.item(cmd::UNDO, "&Undo   Ctrl+Z");
        e.item(cmd::REDO, "&Redo   Ctrl+Y");
        e.separator();
        e.item(cmd::DELETE, "&Delete   Del");
        e.item(cmd::FLIP, "&Flip add/subtract");
        e.item(cmd::FORWARD, "Move &later in order");
        e.item(cmd::BACK, "Move &earlier in order");
        e.separator();
        e.item(cmd::SLICE, "&Apply slice   Enter");
        e.item(cmd::SLICE_SIDE, "Slice &keeps...   K");
        e.item(cmd::CARVE, "&Carve with brush   Ctrl+Shift+C");
        e.item(cmd::EXTRUDE, "Extrude face &out   Ctrl+E");
        e.item(cmd::EXTRUDE_IN, "Extrude face &in   Ctrl+Shift+E");
    });
    m.submenu(MenuId::new(102), "&View", |v| {
        v.item(cmd::FIT, "&Fit views to level");
        v.item(cmd::RESET_CAMERA, "&Reset 3D camera");
        v.separator();
        v.item(cmd::LAYOUT_CLASSIC, "&Classic layout");
        v.item(cmd::LAYOUT_QUAD, "&Quad layout (2 x 2)");
    });
    m.submenu(MenuId::new(103), "&Play", |p| {
        p.item(cmd::PLAY, "&Play   F5");
    });
}

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
            cmd::PLAY => self.play_command(),
            cmd::RESET_CAMERA => {
                self.studio.reset_camera(&self.state.borrow().doc);
            }
            cmd::FIT => self.refit(),
            _ if self.layout_command(id) => {}
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
            cmd::SLICE => {
                st.apply_slice();
            }
            cmd::SLICE_SIDE => st.cycle_keep(),
            cmd::CARVE => {
                st.carve_selected();
            }
            cmd::EXTRUDE => {
                st.extrude_faces(1.0);
            }
            cmd::EXTRUDE_IN => {
                st.extrude_faces(-1.0);
            }
            _ => {}
        }
    }

    /// Replaces the document (New, Open, showcase).
    fn install(&mut self, doc: Document, path: Option<PathBuf>, message: &str) {
        self.studio.reset(&doc);
        self.studio.set_cache_level(path.as_deref());
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
                let lit = self.studio.save_lightmap(&path, &doc);
                self.saved = doc;
                self.path = Some(path);
                lit.map_or(msg.clone(), |l| format!("{msg}; {l}"))
            }
            Err(e) => e,
        };
        self.state.borrow_mut().message = message;
    }

    /// F5: play the level as it is, as soon as the preview has caught up.
    /// Lighting still being baked arrives during the game.
    fn play_command(&mut self) {
        let doc = self.state.borrow().doc.clone();
        if self.studio.is_current(&doc) {
            self.begin_play();
            return;
        }
        let message = match self.studio.play_when_ready(&doc) {
            Ok(()) => "Preparing the level...".to_string(),
            Err(e) => e,
        };
        self.state.borrow_mut().message = message;
    }
}
