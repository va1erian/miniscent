//! `miniscent`: the game executable.

use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use miniscent_game::agent::{self, TestOutput};
use miniscent_game::app::{self, RunOptions, RunReport};
use miniscent_game::cli::{self, Options, Renderer, USAGE};
use miniscent_game::scripts::ScriptWatcher;
use miniscent_game::{GameSession, SessionConfig, assets, levels, lightcache, scripts};
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

/// Frame cap of a window the software renderer draws.
const SOFTWARE_FPS: f32 = 60.0;
/// Frame cap of a window the GPU draws: the simulation already steps at 120 Hz,
/// and high-refresh displays show every frame.
const ACCELERATED_FPS: f32 = 120.0;

/// The frame cap for a window: as asked with `--fps`, else by what draws it.
fn frame_cap(asked: Option<u32>, accelerated: bool) -> f32 {
    match asked {
        Some(fps) => fps as f32,
        None if accelerated => ACCELERATED_FPS,
        None => SOFTWARE_FPS,
    }
}

fn run_options(o: &Options, cap: Option<f32>) -> RunOptions {
    RunOptions {
        base: o.res,
        adaptive: o.adaptive && !o.headless(),
        cap_fps: cap,
        autoclose: o.autoclose_ms.map(Duration::from_millis),
        shot_times: o.shots_at.clone(),
        shot_stem: o.screenshot.clone().or_else(|| {
            (!o.shots_at.is_empty()).then(|| Path::new("target/shots/play.png").to_path_buf())
        }),
        watch: None,
    }
}

/// What draws the world in this run, as `--renderer` and the build say.
fn world(o: &Options) -> Result<Box<dyn app::World>, String> {
    match o.renderer() {
        Renderer::Software => Ok(Box::new(app::Software)),
        #[cfg(feature = "hardware")]
        choice => Ok(Box::new(miniscent_game::gpu_world::GpuWorld::new(
            choice == Renderer::Hardware,
        ))),
        #[cfg(not(feature = "hardware"))]
        Renderer::Auto => Ok(Box::new(app::Software)),
        #[cfg(not(feature = "hardware"))]
        Renderer::Hardware => Err("built without the `hardware` feature".into()),
    }
}

/// What draws the world in a window: the GPU, showing frames there itself where
/// the platform gives it the window.
#[cfg(feature = "desktop")]
fn desktop_world(
    o: &Options,
    platform: &mscent_platform::DesktopPlatform,
) -> Result<Box<dyn app::World>, String> {
    #[cfg(feature = "hardware")]
    if o.renderer() != Renderer::Software
        && let Some(window) = mscent_platform::Platform::gpu_window(platform)
    {
        let strict = o.renderer() == Renderer::Hardware;
        let world = miniscent_game::gpu_world::GpuWorld::new(strict).in_window(window);
        return Ok(Box::new(world));
    }
    let _ = platform;
    world(o)
}

/// `report`, or why the run stopped short.
fn finished(report: RunReport) -> Result<RunReport, String> {
    match &report.stopped {
        Some(why) => Err(format!("the GPU cannot draw this: {why}")),
        None => Ok(report),
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
    let mut world = world(o)?;
    let opts = run_options(o, None);
    let report = finished(app::run_with(&mut platform, session, &opts, world.as_mut()))?;
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
    let mut world = desktop_world(o, &platform)?;
    // A world that shows itself in the window is the GPU's.
    let opts = RunOptions {
        watch: o.level.as_deref().map(ScriptWatcher::new),
        ..run_options(o, Some(frame_cap(o.fps, world.shows())))
    };
    finished(app::run_with(&mut platform, session, &opts, world.as_mut()))
}

#[cfg(not(feature = "desktop"))]
fn run_desktop(_: &Options, _: &mut GameSession) -> Result<RunReport, String> {
    Err("built without the `desktop` feature; use --screenshot, --script or --bench".into())
}

/// The commands that do their one job and exit. `None` if this is not one.
fn agent_command(o: &Options) -> Option<Result<String, String>> {
    let needs_level = |what: &str| format!("{what} needs --level <file.msl>");
    if o.script_api {
        return Some(Ok(agent::script_api()));
    }
    if let Some(path) = &o.dump_level {
        return Some(agent::dump_level(path));
    }
    if let Some((from, to)) = &o.pack_level {
        return Some(agent::pack_level(from, to));
    }
    if o.check {
        let level = o.level.as_deref().ok_or_else(|| needs_level("--check"));
        return Some(level.and_then(agent::check));
    }
    let test = o.test.as_deref()?;
    let out = TestOutput {
        trace: o.trace.clone(),
        shots_dir: o.shots_dir.clone().unwrap_or_else(|| "target/shots".into()),
        assets: o.assets.clone(),
    };
    let level = o.level.as_deref().ok_or_else(|| needs_level("--test"));
    Some(level.and_then(|level| agent::run_test(level, test, &out)))
}

fn real_main() -> Result<(), String> {
    let o = cli::parse(std::env::args().skip(1))?;
    if o.help {
        print!("{USAGE}");
        return Ok(());
    }
    if let Some(result) = agent_command(&o) {
        println!("{}", result?);
        return Ok(());
    }
    let level = level_of(&o)?;
    let materials = assets::load_materials(o.assets.as_deref())?;
    let scripts = o.level.as_deref().map(scripts::for_level).transpose()?;
    let scripts = scripts.flatten();
    let cfg = SessionConfig {
        quality: o.quality,
        scripted: scripts.is_some(),
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
    if let Some(scripts) = &scripts {
        for warning in scripts::start(&mut session, scripts)? {
            eprintln!("script warning: {warning}");
        }
    }
    session.set_chase(o.chase);
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
