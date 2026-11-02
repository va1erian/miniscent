//! Menu commands: files, play and the edit operations.

use std::path::{Path, PathBuf};

use xui::prelude::Ui;
use xui::widget::MenuId;

use super::app::{Editor, Msg, cmd};
use super::level_dialog;
use crate::files;
use crate::model::Document;
use crate::studio::Mode;

/// Fog the dialog starts from for a level without a world entity.
const DEFAULT_FOG: (f32, f32) = (4.0, 26.0);

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
        e.item(cmd::MAKE_MOVER, "Make &mover");
        e.item(cmd::FORWARD, "Move &later in order");
        e.item(cmd::BACK, "Move &earlier in order");
        e.separator();
        e.item(cmd::SLICE, "&Apply slice   Enter");
        e.item(cmd::SLICE_SIDE, "Slice &keeps...   K");
        e.item(cmd::CARVE, "&Carve with brush   Ctrl+Shift+C");
        e.item(cmd::EXTRUDE, "Extrude face &out   Ctrl+E");
        e.item(cmd::EXTRUDE_IN, "Extrude face &in   Ctrl+Shift+E");
        e.separator();
        e.item(cmd::LEVEL_SETTINGS, "Level &settings...");
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
    m.submenu(MenuId::new(104), "&Texture", |t| {
        t.item(cmd::TEX_OPEN, "Texture &Studio");
        t.item(cmd::TEX_CLOSE, "&Back to the level   Esc");
        t.separator();
        t.item(cmd::TEX_NEW, "&New material...");
        t.item(cmd::TEX_APPLY, "&Apply to the level");
        t.item(cmd::TEX_SAVE, "&Save material   Ctrl+S");
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
                st.commit_pending();
                st.delete_selected();
            }
            cmd::FLIP => {
                st.flip_selected();
            }
            cmd::MAKE_MOVER => {
                st.make_mover();
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

    /// Edit > Level settings: asks for the fog and applies it to the world
    /// entity.
    pub(super) fn level_settings(&mut self, ui: &Ui<Msg>) {
        if self.studio.mode == Mode::Play {
            return;
        }
        let current = self.state.borrow().fog().unwrap_or(DEFAULT_FOG);
        if let Some((start, end)) = level_dialog::ask_fog(ui, current) {
            self.state.borrow_mut().set_fog(start, end);
            self.refresh();
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
        // What is on screen is what is saved, with an undo step to say so.
        self.state.borrow_mut().commit_pending();
        let doc = self.state.borrow().doc.clone();
        let message = match files::save(&doc, &path) {
            Ok(msg) => {
                files::remember(&path);
                let lit = self.studio.save_lightmap(&path, &doc);
                let mut doc = doc;
                doc.set_level_path(Some(path.clone()));
                self.state
                    .borrow_mut()
                    .doc
                    .set_level_path(Some(path.clone()));
                let unsaved = self
                    .tex
                    .studio
                    .borrow()
                    .unsaved_among(&doc.materials_used());
                self.saved = doc;
                self.path = Some(path);
                let said = lit.map_or(msg.clone(), |l| format!("{msg}; {l}"));
                match unsaved.is_empty() {
                    true => said,
                    // The level is saved all the same: the file is right, the
                    // material is what is missing, and Save material is the cure.
                    false => format!(
                        "{said}. WARNING: it uses {}, applied in the Texture Studio but not \
                         saved: next session those faces draw grey, or another material",
                        unsaved.join(", ")
                    ),
                }
            }
            Err(e) => e,
        };
        self.state.borrow_mut().message = message;
    }

    /// F5: play the level as it is, as soon as the preview has caught up.
    /// Lighting still being baked arrives during the game.
    fn play_command(&mut self) {
        self.state.borrow_mut().commit_pending();
        let doc = self.state.borrow().doc.clone();
        if self.studio.playable_now(&doc) {
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
