//! The miniscent level editor. `--showcase` (or MSCENT_EDITOR_SHOWCASE=1)
//! starts with the demo level loaded; `XUI_SNAPSHOT=<dir>` renders it
//! headlessly to PNGs.

mod model;
mod stubs;
mod ui;

use model::Document;
use ui::Editor;

fn main() -> xui::backend::Result<()> {
    let showcase = std::env::args().any(|a| a == "--showcase")
        || std::env::var_os("MSCENT_EDITOR_SHOWCASE").is_some();
    xui::app("miniscent editor").size(1280, 800).run(move |ui| {
        let doc = if showcase {
            Document::from_level(&mscent_world::showcase_level())
        } else {
            Document::new()
        };
        Editor::build(ui, doc)
    })
}
