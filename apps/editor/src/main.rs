//! The miniscent level editor.
//!
//! * `mscent-editor [level.msl]` opens a level; `--showcase` (or
//!   MSCENT_EDITOR_SHOWCASE=1) starts with the demo level.
//! * `XUI_SNAPSHOT=<dir>` renders the window headlessly to PNGs.
//! * `--selftest <dir>` runs a scripted session (load the showcase, build,
//!   snapshot, play 120 frames, return, edit, snapshot) and exits 0 on success.

mod build;
mod files;
mod model;
mod selftest;
mod studio;
mod ui;

use model::Document;
use ui::Editor;

fn main() -> xui::backend::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        let Some(dir) = args.get(i + 1) else {
            eprintln!("--selftest needs an output directory");
            std::process::exit(2);
        };
        std::process::exit(selftest::run(std::path::Path::new(dir)));
    }
    let showcase = args.iter().any(|a| a == "--showcase")
        || std::env::var_os("MSCENT_EDITOR_SHOWCASE").is_some();
    let path = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map(std::path::PathBuf::from);
    xui::app("miniscent editor").size(1280, 800).run(move |ui| {
        if let Some(p) = &path {
            return match files::load(p) {
                Ok(doc) => Editor::build_from(ui, doc, Some(p.clone()), None),
                Err(e) => Editor::build_from(ui, Document::new(), None, Some(e)),
            };
        }
        let doc = if showcase {
            Document::from_level(&mscent_world::showcase_level())
        } else {
            Document::new()
        };
        Editor::build(ui, doc)
    })
}
