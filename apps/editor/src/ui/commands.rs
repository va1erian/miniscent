//! Menu commands: files, play and the edit operations.

use std::path::{Path, PathBuf};

use xui::prelude::Ui;
use xui::widget::MenuId;

use super::app::{Editor, Msg};
use super::level_dialog;
use super::script_actions::{KIT_MENU, SAMPLE_MENU};
use crate::files;
use crate::model::Document;
use crate::script::kit::KIT;
use crate::script::samples::SAMPLES;
use crate::studio::Mode;

/// Menu command ids.
pub mod cmd {
    use xui::widget::MenuId;
    pub const NEW: MenuId = MenuId::new(1);
    pub const OPEN: MenuId = MenuId::new(2);
    pub const SAVE: MenuId = MenuId::new(3);
    pub const PLAY: MenuId = MenuId::new(5);
    pub const SHOWCASE: MenuId = MenuId::new(6);
    pub const SAVE_AS: MenuId = MenuId::new(7);
    pub const UNDO: MenuId = MenuId::new(10);
    pub const REDO: MenuId = MenuId::new(11);
    pub const MAKE_MOVER: MenuId = MenuId::new(26);
    pub const DELETE: MenuId = MenuId::new(12);
    pub const FLIP: MenuId = MenuId::new(13);
    pub const FORWARD: MenuId = MenuId::new(14);
    pub const BACK: MenuId = MenuId::new(15);
    pub const SLICE: MenuId = MenuId::new(16);
    pub const SLICE_SIDE: MenuId = MenuId::new(17);
    pub const CARVE: MenuId = MenuId::new(18);
    pub const EXTRUDE: MenuId = MenuId::new(19);
    pub const EXTRUDE_IN: MenuId = MenuId::new(22);
    pub const FIT: MenuId = MenuId::new(20);
    pub const RESET_CAMERA: MenuId = MenuId::new(21);
    pub const LAYOUT_CLASSIC: MenuId = MenuId::new(23);
    pub const LAYOUT_QUAD: MenuId = MenuId::new(24);
    pub const LEVEL_SETTINGS: MenuId = MenuId::new(25);
    pub const TEX_OPEN: MenuId = MenuId::new(60);
    pub const TEX_CLOSE: MenuId = MenuId::new(61);
    pub const TEX_NEW: MenuId = MenuId::new(62);
    pub const TEX_APPLY: MenuId = MenuId::new(63);
    pub const TEX_SAVE: MenuId = MenuId::new(64);
    pub const SCRIPT_PANE: MenuId = MenuId::new(27);
    pub const GUIDE: MenuId = MenuId::new(28);
    pub const SCRIPT_WINDOW: MenuId = MenuId::new(29);
}

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
    m.submenu(MenuId::new(108), "&Texture", |t| {
        t.item(cmd::TEX_OPEN, "Texture &Studio");
        t.item(cmd::TEX_CLOSE, "&Back to the level   Esc");
        t.separator();
        t.item(cmd::TEX_NEW, "&New material...");
        t.item(cmd::TEX_APPLY, "&Apply to the level");
        t.item(cmd::TEX_SAVE, "&Save material   Ctrl+S");
    });
    m.submenu(MenuId::new(104), "S&cript", |s| {
        s.item(cmd::SCRIPT_PANE, "Show or hide &script   F6");
        s.item(cmd::SCRIPT_WINDOW, "Script in its own &window, or docked");
        s.separator();
        s.submenu(MenuId::new(105), "&Add entity", |a| {
            for (id, entry) in KIT_MENU.iter().zip(&KIT) {
                a.item(*id, entry.label);
            }
        });
        s.submenu(MenuId::new(106), "Open s&ample", |a| {
            for (id, sample) in SAMPLE_MENU.iter().zip(&SAMPLES) {
                a.item(*id, sample.title);
            }
        });
    });
    m.submenu(MenuId::new(107), "&Help", |h| {
        h.item(cmd::GUIDE, "Scripting &guide   F7");
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
            cmd::SAVE => self.save_command(),
            cmd::SAVE_AS => self.save_dialog.open(),
            cmd::PLAY => self.play_command(),
            cmd::RESET_CAMERA => {
                self.studio.reset_camera(&self.state.borrow().doc);
            }
            cmd::FIT => self.refit(),
            cmd::SCRIPT_PANE => self.toggle_script(),
            cmd::GUIDE => self.show_guide(),
            cmd::SCRIPT_WINDOW => {
                let floats = !self.script.floats();
                self.set_script_floating(floats);
            }
            _ if self.layout_command(id) => {}
            _ if self.script_list_command(id) => {}
            _ => self.edit_command(id),
        }
    }

    /// The Script pane's Save is File > Save: a level and its script go together.
    pub(super) fn save_command(&mut self) {
        match self.path.clone() {
            Some(p) => self.save_path(p),
            None => self.save_dialog.open(),
        }
    }

    /// The Script menu's lists; `false` for any other command.
    fn script_list_command(&mut self, id: MenuId) -> bool {
        if let Some(i) = KIT_MENU.iter().position(|m| *m == id) {
            self.pick_kit(i);
        } else if let Some(i) = SAMPLE_MENU.iter().position(|m| *m == id) {
            self.open_sample(i);
        } else {
            return false;
        }
        true
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

    /// Edit > Level settings: opens the window that asks for the fog, on what the
    /// level has now, in place of one that may be showing something older.
    pub(super) fn level_settings(&mut self, ui: &Ui<Msg>) {
        if self.studio.mode == Mode::Play {
            return;
        }
        self.close_fog_window();
        let current = self.state.borrow().fog().unwrap_or(DEFAULT_FOG);
        match level_dialog::open(ui, current) {
            Ok(window) => *self.fog_window.borrow_mut() = Some(window),
            Err(e) => {
                self.state.borrow_mut().message = format!("Cannot open Level settings: {e:?}");
            }
        }
    }

    pub(super) fn close_fog_window(&mut self) {
        if let Some(window) = self.fog_window.borrow_mut().take() {
            window.close();
        }
    }

    /// The window's OK. The level may have started playing meanwhile, and then the
    /// document is frozen.
    pub(super) fn set_fog(&mut self, start: f32, end: f32) {
        if self.studio.mode != Mode::Play {
            self.state.borrow_mut().set_fog(start, end);
        }
    }

    /// Replaces the document (New, Open, showcase).
    fn install(&mut self, doc: Document, path: Option<PathBuf>, message: &str) {
        self.close_fog_window();
        self.studio.reset(&doc);
        self.studio.set_cache_level(path.as_deref());
        self.saved = doc.clone();
        self.path = path;
        {
            let mut st = self.state.borrow_mut();
            st.load(doc);
            st.message = message.into();
        }
        if !self.state.borrow().script.text().is_empty() {
            self.script.set_open(true);
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
                let script = self.save_script(&path);
                if matches!(script, Ok(true)) {
                    // F5 straight after must not find the old build current.
                    self.studio.script_changed();
                }
                self.path = Some(path);
                let said = lit.map_or(msg.clone(), |l| format!("{msg}; {l}"));
                let said = match script {
                    Ok(true) => format!("{said}; script saved"),
                    Ok(false) => said,
                    Err(e) => format!("{said}; THE SCRIPT WAS NOT SAVED: {e}"),
                };
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
}
