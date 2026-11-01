//! `miniscent`: the game executable.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use miniscent_game::app::{self, RunOptions, RunReport};
use miniscent_game::cli::{self, Options, USAGE};
use miniscent_game::{GameSession, SessionConfig, assets, levels};
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::png_encode::encode_png;
use mscent_world::{Level, showcase_level};

/// Virtual frame time of headless runs.
const HEADLESS_DT: Duration = Duration::from_micros(16_667);
/// A script for `--bench` without `--script`: fly forward while turning.
const BENCH_SCRIPT: &str = "at 0s hold W\nat 0s mouse 900 0 over 5s\n";

fn level_of(o: &Options) -> Result<Level, String> {
    match (&o.level, o.corridor) {
        (Some(path), _) => assets::load_level(path),
        (None, true) => Ok(levels::corridor_level()),
        (None, false) => Ok(showcase_level()),
    }
}

fn script_of(o: &Options) -> Result<Script, String> {
    let text = match (&o.script, o.bench) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?,
        (None, true) => BENCH_SCRIPT.to_string(),
        (None, false) => String::new(),
    };
    Script::parse(&text).map_err(|e| e.to_string())
}

fn run_options(o: &Options, cap: bool) -> RunOptions {
    RunOptions {
        base: o.res,
        adaptive: o.adaptive && !o.headless(),
        cap_fps: cap.then_some(60.0),
        autoclose: o.autoclose_ms.map(Duration::from_millis),
        shot_times: o.shots_at.clone(),
        shot_stem: o.screenshot.clone().or_else(|| {
            (!o.shots_at.is_empty()).then(|| Path::new("target/shots/play.png").to_path_buf())
        }),
    }
}

fn run_headless(o: &Options, session: &mut GameSession) -> Result<RunReport, String> {
    let frames = o.frames.unwrap_or(if o.bench { 300 } else { 120 });
    let mut platform = HeadlessPlatform::new(o.res.0, o.res.1, HEADLESS_DT, script_of(o)?)
        .with_frame_limit(frames);
    let report = app::run(&mut platform, session, &run_options(o, false));
    if let (Some(path), Some(frame)) = (&o.screenshot, platform.last_frame()) {
        let png = encode_png(frame.width, frame.height, &frame.pixels)
            .ok_or("could not encode the screenshot")?;
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, png).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(report)
}

#[cfg(feature = "desktop")]
fn run_desktop(o: &Options, session: &mut GameSession) -> Result<RunReport, String> {
    use mscent_platform::{DesktopConfig, DesktopPlatform};
    let mut platform = DesktopPlatform::new(DesktopConfig {
        title: "miniscent".into(),
        width: o.res.0 * o.scale,
        height: o.res.1 * o.scale,
    })
    .map_err(|e| format!("cannot open a window: {e}"))?;
    Ok(app::run(&mut platform, session, &run_options(o, true)))
}

#[cfg(not(feature = "desktop"))]
fn run_desktop(_: &Options, _: &mut GameSession) -> Result<RunReport, String> {
    Err("built without the `desktop` feature; use --screenshot, --script or --bench".into())
}

fn real_main() -> Result<(), String> {
    let o = cli::parse(std::env::args().skip(1))?;
    if o.help {
        print!("{USAGE}");
        return Ok(());
    }
    let level = level_of(&o)?;
    let materials = assets::load_materials(o.assets.as_deref())?;
    let cfg = SessionConfig {
        quality: o.quality,
        ..SessionConfig::default()
    };
    let started = std::time::Instant::now();
    let mut session = GameSession::new(&level, materials, &cfg).map_err(|e| e.to_string())?;
    println!("level ready in {:.2}s", started.elapsed().as_secs_f32());
    let report = if o.headless() {
        run_headless(&o, &mut session)?
    } else {
        run_desktop(&o, &mut session)?
    };
    if o.bench || o.headless() {
        println!(
            "{} frames: {:.2} ms/frame avg, {:.2} p95, {:.2} max, scale {}",
            report.frames, report.avg_ms, report.p95_ms, report.max_ms, report.final_scale
        );
    }
    for shot in &report.shots {
        println!("wrote {}", shot.display());
    }
    println!("final phase {:?}", session.phase());
    Ok(())
}

fn main() -> ExitCode {
    match real_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("miniscent: {e}");
            ExitCode::FAILURE
        }
    }
}
