//! The miniscent level editor.
//!
//! * `mscent-editor [level.msl]` opens a level; `--showcase` (or
//!   MSCENT_EDITOR_SHOWCASE=1) starts with the demo level.
//! * `--slow-play-ms <n>` (hidden) makes each rendered frame take n ms longer.
//! * `XUI_SNAPSHOT=<dir>` renders the window headlessly to PNGs.
//! * `--selftest <dir>` runs a scripted session (load the showcase, snapshot,
//!   play 120 frames, return, edit, snapshot) and exits 0 on success.
//! * `--ui-timing [dir]` prints what the UI thread's everyday work costs.

mod build;
mod files;
mod materials;
mod model;
mod script;
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
    if let Some(i) = args.iter().position(|a| a == "--ui-timing") {
        let dir = args
            .get(i + 1)
            .map_or("target/shots/timing", String::as_str);
        std::process::exit(selftest::ui_timing(std::path::Path::new(dir)));
    }
    // Hidden: make every frame take this many ms longer, to see that a slow
    // game does not slow the UI. MSCENT_SLOW_PLAY_MS does the same.
    let slow = args
        .iter()
        .position(|a| a == "--slow-play-ms")
        .and_then(|i| args.get(i + 1).cloned())
        .or_else(|| std::env::var("MSCENT_SLOW_PLAY_MS").ok())
        .and_then(|v| v.parse::<u64>().ok());
    if let Some(ms) = slow {
        studio::worker::set_slow_ms(ms);
    }
    let showcase = args.iter().any(|a| a == "--showcase")
        || std::env::var_os("MSCENT_EDITOR_SHOWCASE").is_some();
    let path = args
        .iter()
        .enumerate()
        .find(|(i, a)| {
            let value_of_flag = *i > 0 && args[i - 1] == "--slow-play-ms";
            !a.starts_with("--") && !value_of_flag
        })
        .map(|(_, a)| a)
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
