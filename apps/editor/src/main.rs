//! The miniscent level editor.
//!
//! * `mscent-editor [level.msl]` opens a level; `--showcase` (or
//!   MSCENT_EDITOR_SHOWCASE=1) starts with the demo level.
//! * `--slow-play-ms <n>` (hidden) makes each rendered frame take n ms longer.
//! * `XUI_SNAPSHOT=<dir>` renders the window headlessly to PNGs.
//! * `--selftest <dir>` runs a scripted session (load the showcase, snapshot,
//!   play 120 frames, return, edit, snapshot) and exits 0 on success.
//! * `--bake-texture <recipe.mst> <dir>` writes a recipe's maps as PNG files, and
//!   adds the material to `<dir>/materials.txt` if there is one.
//! * `--ui-timing [dir] [WxH]` prints what the UI thread's everyday work costs,
//!   in a window of 1280x800 or `WxH` pixels.
//! * `--canvas` uses the software backend where Direct2D is the default.
//! * `--live-timing [level.msl]` opens the real window on the voxel cave (or the
//!   level given), drags, sculpts and flies on a script, prints how often each
//!   part of the window painted, and quits.

// `--selftest` and `--ui-timing` need the canvas backend, so without it the probes and
// accessors they alone use would warn as dead code.
#![cfg_attr(not(feature = "canvas"), allow(dead_code, unused_imports))]

mod build;
mod files;
mod materials;
mod model;
mod script;
#[cfg(feature = "canvas")]
mod selftest;
mod studio;
mod texbake;
mod texstudio;
mod ui;

use model::Document;
use ui::Editor;

fn main() -> xui::backend::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    studio::gpu_view::set_wanted(!args.iter().any(|a| a == "--software"));
    #[cfg(feature = "canvas")]
    if let Some(i) = args.iter().position(|a| a == "--selftest") {
        let Some(dir) = args.get(i + 1) else {
            eprintln!("--selftest needs an output directory");
            std::process::exit(2);
        };
        std::process::exit(selftest::run(std::path::Path::new(dir)));
    }
    if let Some(i) = args.iter().position(|a| a == "--bake-texture") {
        let (Some(recipe), Some(dir)) = (args.get(i + 1), args.get(i + 2)) else {
            eprintln!("--bake-texture needs a recipe and an output directory");
            std::process::exit(2);
        };
        if let Err(e) = texbake::run(std::path::Path::new(recipe), std::path::Path::new(dir)) {
            eprintln!("{e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    #[cfg(feature = "canvas")]
    if let Some(i) = args.iter().position(|a| a == "--ui-timing") {
        let dir = args
            .get(i + 1)
            .map_or("target/shots/timing", String::as_str);
        let Some(size) = selftest::window_size(args.get(i + 2).map(String::as_str)) else {
            eprintln!("--ui-timing: the window size is WxH, as in 3840x2160");
            std::process::exit(2);
        };
        std::process::exit(selftest::ui_timing(std::path::Path::new(dir), size));
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
    let live = args.iter().any(|a| a == "--live-timing");
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
    let path = path.or_else(|| live.then(live_level));
    let make = move |ui: &mut xui::app::Ui<ui::Msg>| {
        let mut editor = open(ui, path.as_deref(), showcase)?;
        editor.start_ticks(ui);
        if live {
            editor.start_live(ui);
        }
        Ok(editor)
    };
    launch(make, args.iter().any(|a| a == "--canvas"))
}

/// The level `--live-timing` opens when given none: the voxel cave.
fn live_level() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/voxel-cave.msl")
}

/// The editor on `path`, the showcase or an empty level.
fn open(
    ui: &mut xui::app::Ui<ui::Msg>,
    path: Option<&std::path::Path>,
    showcase: bool,
) -> xui::backend::Result<Editor> {
    if let Some(p) = path {
        return match files::load(p) {
            Ok(doc) => Editor::build_from(ui, doc, Some(p.to_path_buf()), None),
            Err(e) => Editor::build_from(ui, Document::new(), None, Some(e)),
        };
    }
    let doc = if showcase {
        Document::from_level(&mscent_world::showcase_level())
    } else {
        Document::new()
    };
    Editor::build(ui, doc)
}

/// Opens the editor window: on Direct2D where it is built in, unless
/// `--canvas` asks for the software backend.
fn launch<F>(make: F, canvas: bool) -> xui::backend::Result<()>
where
    F: FnOnce(&mut xui::app::Ui<ui::Msg>) -> xui::backend::Result<Editor>,
{
    let app = xui::app("miniscent editor").size(1280, 800);
    #[cfg(all(windows, feature = "d2d"))]
    if !canvas {
        return app
            .backend(std::rc::Rc::new(xui_d2d::D2dBackend::new()))
            .run(make);
    }
    #[cfg(feature = "canvas")]
    {
        let _ = canvas;
        app.run(make)
    }
    #[cfg(not(feature = "canvas"))]
    {
        let _ = (app, make, canvas);
        Err(xui::backend::BackendError::Unsupported(
            "no backend built in",
        ))
    }
}
