//! `miniscent`: the game executable.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use miniscent_game::app::{self, RunOptions, RunReport};
use miniscent_game::cli::{self, Options, USAGE};
use miniscent_game::{GameSession, SessionConfig, assets, levels, lightcache};
use mscent_platform::{HeadlessPlatform, Script};
use mscent_render::png_encode::encode_png;
use mscent_render::{BakeCache, LevelRenderer};
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

/// `--bake`: compile and bake only, leaving the cache file for later runs.
fn bake_only(level: &Level, o: &Options, cache: &mut BakeCache) -> Result<(), String> {
    let materials = assets::load_materials(o.assets.as_deref())?;
    let cfg = SessionConfig::default();
    let r = LevelRenderer::from_level_cached(level, materials, &cfg.build, cache)
        .map_err(|e| format!("bake failed: {e:?}"))?;
    let map = r.lightmap();
    let atlas = map.atlas();
    println!(
        "lightmap: {} faces, atlas {}x{} ({} luxels)",
        map.projections().len(),
        atlas.width(),
        atlas.height(),
        u64::from(atlas.width()) * u64::from(atlas.height())
    );
    if let Some(rep) = cache.last_report() {
        println!("{}", rep.describe());
        if let Some(len) = rep.path.as_ref().and_then(|p| std::fs::metadata(p).ok()) {
            println!("lightmap: cache file {} bytes", len.len());
        }
    }
    Ok(())
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
    let script = script_of(o)?;
    // Without --frames a script runs to its last action (plus a margin).
    let script_frames =
        script.actions().iter().map(|a| a.0).fold(0.0, f64::max) / HEADLESS_DT.as_secs_f64();
    let default_frames = if script_frames > 0.0 {
        script_frames.ceil() as u64 + 30
    } else if o.bench {
        300
    } else {
        120
    };
    let frames = o.frames.unwrap_or(default_frames);
    let mut platform =
        HeadlessPlatform::new(o.res.0, o.res.1, HEADLESS_DT, script).with_frame_limit(frames);
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
    let name = if o.corridor { "corridor" } else { "showcase" };
    let mut cache = lightcache::open(o.level.as_deref(), name, o.no_cache, o.rebake);
    if o.bake {
        return bake_only(&level, &o, &mut cache);
    }
    let started = std::time::Instant::now();
    let mut session =
        GameSession::new_cached(&level, materials, &cfg, &mut cache).map_err(|e| e.to_string())?;
    println!("level ready in {:.2}s", started.elapsed().as_secs_f32());
    if let Some(rep) = cache.last_report() {
        println!("{}", rep.describe());
    }
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
    let at = session.game().player.pos();
    println!(
        "final phase {:?} at ({:.1}, {:.1}, {:.1})",
        session.phase(),
        at.x,
        at.y,
        at.z
    );
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
