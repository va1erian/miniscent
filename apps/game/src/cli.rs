//! Command line parsing. No dependencies: the options are few.

use std::path::PathBuf;

use mscent_render::Quality;

pub const USAGE: &str = "\
miniscent [options]

  --level <file.msl>       play this level (default: the built-in showcase);
                           a bare <file.msl> argument does the same
  --bake                   only bake the level's lightmap, write the cache
                           (<level>.msl.lit, or the per-user cache dir for
                           built-in levels), print stats and exit
  --no-cache               neither read nor write the lightmap cache
  --rebake                 ignore the cached lightmap, bake, overwrite it
  --corridor               play the built-in corridor level instead
  --quality low|medium|high  lighting model (default high)
  --scale <n>              desktop window = internal resolution * n (default 2)
  --res <W>x<H>            internal resolution at full quality (default 640x360)
  --assets <dir>           asset root holding textures/ (default: searched)
  --screenshot <out.png>   headless: run, then write the last frame
  --frames <n>             headless: frames to run (default 120)
  --shots-at <t,t,..>      write <screenshot stem>_<t>s.png at those seconds
  --script <file>          headless: scripted input (see mscent-platform)
  --bench                  headless: print ms/frame for the run
  --no-adaptive            keep the internal resolution fixed
  --mute                   desktop: play no sound
  --fps <n>                desktop: frame rate cap, 15..240 (default 120 when
                           the GPU draws the window, 60 otherwise)
  --renderer auto|software|hardware
                           what draws the world: the GPU where it can (auto,
                           the default in a window), software (the default
                           headless, whose pictures are the same everywhere),
                           or the GPU and nothing else, failing with why
  --chase                  put the camera behind the player's ship and draw it (the
                           hardware renderer only; cosmetic)
  --autoclose-ms <n>       desktop: quit after n milliseconds
  --check                  with --level: compile the level's script, run its top
                           level and list every name; fails on any error
  --test <file.st>         with --level: play with no window until this test
                           script ends; fails on a failed assert or script error
  --trace <out.jsonl>      with --test: write what happened, one JSON object a line
  --shots-dir <dir>        with --test: where shot: writes (default target/shots)
  --script-api             print the language and every function a script can call
  --dump-level <file.msl>  print a level as text
  --pack-level <in> <out>  read a level as text and write it as a .msl
  --scale-guide <n|file>   print what doors, ceilings and climbs a level needs for
                           the ship at n times its stock size (or a level's own
                           size, from its scale line) and for the pilot on foot
  --help
";

/// What draws the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Renderer {
    /// The GPU, and software where it cannot.
    Auto,
    Software,
    /// The GPU only: a run it cannot draw stops with the reason.
    Hardware,
}

fn renderer(s: &str) -> Result<Renderer, String> {
    match s {
        "auto" => Ok(Renderer::Auto),
        "software" => Ok(Renderer::Software),
        "hardware" => Ok(Renderer::Hardware),
        _ => Err(format!(
            "unknown renderer `{s}` (auto, software or hardware)"
        )),
    }
}

/// Parsed command line.
#[derive(Clone, Debug)]
pub struct Options {
    pub level: Option<PathBuf>,
    pub corridor: bool,
    pub quality: Quality,
    pub scale: u32,
    pub res: (u32, u32),
    pub assets: Option<PathBuf>,
    pub screenshot: Option<PathBuf>,
    pub frames: Option<u64>,
    pub shots_at: Vec<f64>,
    pub script: Option<PathBuf>,
    pub bench: bool,
    pub bake: bool,
    pub no_cache: bool,
    pub rebake: bool,
    pub adaptive: bool,
    pub mute: bool,
    /// `--fps`: the desktop frame cap; `None` for the renderer's default.
    pub fps: Option<u32>,
    pub autoclose_ms: Option<u64>,
    pub help: bool,
    pub check: bool,
    pub test: Option<PathBuf>,
    pub trace: Option<PathBuf>,
    pub shots_dir: Option<PathBuf>,
    pub script_api: bool,
    pub dump_level: Option<PathBuf>,
    /// `--scale-guide`: a ship size, or a level to read it from.
    pub scale_guide: Option<String>,
    pub pack_level: Option<(PathBuf, PathBuf)>,
    /// `None` for the default: `auto` in a window, `software` headless.
    pub renderer: Option<Renderer>,
    /// `--chase`: a third-person camera, to look at the ship.
    pub chase: bool,
}

impl Options {
    /// What draws the world: as asked, or `auto` in a window and software
    /// headless, where a run is to give the same pictures on every machine.
    pub fn renderer(&self) -> Renderer {
        let default = if self.headless() {
            Renderer::Software
        } else {
            Renderer::Auto
        };
        self.renderer.unwrap_or(default)
    }

    /// Whether the run uses the headless backend instead of a window.
    pub fn headless(&self) -> bool {
        self.screenshot.is_some()
            || self.script.is_some()
            || self.bench
            || !self.shots_at.is_empty()
    }
}

fn quality(s: &str) -> Result<Quality, String> {
    match s {
        "low" => Ok(Quality::Low),
        "medium" => Ok(Quality::Medium),
        "high" => Ok(Quality::High),
        _ => Err(format!("unknown quality `{s}` (low, medium or high)")),
    }
}

fn res(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once('x').ok_or("--res wants WxH, e.g. 640x360")?;
    let parse = |v: &str| v.parse::<u32>().ok().filter(|n| (16..=4096).contains(n));
    match (parse(w), parse(h)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err("--res sizes must be 16..4096".into()),
    }
}

/// The internal resolution at full quality when `--res` does not say: the
/// size the HUD is drawn for.
pub const BASE_RES: (u32, u32) = (640, 360);

/// The slowest and fastest frame cap `--fps` takes.
pub const MIN_FPS: u32 = 15;
pub const MAX_FPS: u32 = 240;

/// Parses the arguments after the program name.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut o = Options {
        level: None,
        corridor: false,
        quality: Quality::High,
        scale: 2,
        res: BASE_RES,
        assets: None,
        screenshot: None,
        frames: None,
        shots_at: Vec::new(),
        script: None,
        bench: false,
        bake: false,
        no_cache: false,
        rebake: false,
        adaptive: true,
        mute: false,
        fps: None,
        autoclose_ms: None,
        help: false,
        check: false,
        test: None,
        trace: None,
        shots_dir: None,
        script_api: false,
        dump_level: None,
        scale_guide: None,
        pack_level: None,
        renderer: None,
        chase: false,
    };
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--level" => o.level = Some(value("--level")?.into()),
            "--corridor" => o.corridor = true,
            "--quality" => o.quality = quality(&value("--quality")?)?,
            "--scale" => {
                o.scale = value("--scale")?
                    .parse()
                    .ok()
                    .filter(|n| (1..=8).contains(n))
                    .ok_or("--scale must be 1..8")?;
            }
            "--res" => o.res = res(&value("--res")?)?,
            "--assets" => o.assets = Some(value("--assets")?.into()),
            "--screenshot" => o.screenshot = Some(value("--screenshot")?.into()),
            "--frames" => {
                o.frames = Some(
                    value("--frames")?
                        .parse()
                        .map_err(|_| "--frames must be a number")?,
                );
            }
            "--shots-at" => {
                for t in value("--shots-at")?.split(',') {
                    let t: f64 = t
                        .trim()
                        .parse()
                        .ok()
                        .filter(|t: &f64| t.is_finite() && *t >= 0.0)
                        .ok_or("--shots-at times must be finite and >= 0")?;
                    o.shots_at.push(t);
                }
            }
            "--script" => o.script = Some(value("--script")?.into()),
            "--bench" => o.bench = true,
            "--bake" => o.bake = true,
            "--no-cache" => o.no_cache = true,
            "--rebake" => o.rebake = true,
            "--no-adaptive" => o.adaptive = false,
            "--mute" => o.mute = true,
            "--fps" => {
                o.fps = Some(
                    value("--fps")?
                        .parse()
                        .ok()
                        .filter(|n| (MIN_FPS..=MAX_FPS).contains(n))
                        .ok_or("--fps must be 15..240")?,
                );
            }
            "--chase" => o.chase = true,
            "--renderer" => o.renderer = Some(renderer(&value("--renderer")?)?),
            "--autoclose-ms" => {
                o.autoclose_ms = Some(
                    value("--autoclose-ms")?
                        .parse()
                        .map_err(|_| "--autoclose-ms must be a number")?,
                );
            }
            "--check" => o.check = true,
            "--test" => o.test = Some(value("--test")?.into()),
            "--trace" => o.trace = Some(value("--trace")?.into()),
            "--shots-dir" => o.shots_dir = Some(value("--shots-dir")?.into()),
            "--script-api" => o.script_api = true,
            "--dump-level" => o.dump_level = Some(value("--dump-level")?.into()),
            "--scale-guide" => o.scale_guide = Some(value("--scale-guide")?),
            "--pack-level" => {
                let from = value("--pack-level")?.into();
                let to = value("--pack-level")?.into();
                o.pack_level = Some((from, to));
            }
            "--help" | "-h" => o.help = true,
            file if !file.starts_with('-') && o.level.is_none() => o.level = Some(file.into()),
            other => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
        }
    }
    o.shots_at.sort_by(f64::total_cmp);
    Ok(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Options, String> {
        parse(args.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn defaults_are_the_windowed_showcase() {
        let o = p(&[]).unwrap();
        assert!(!o.headless() && o.adaptive && o.level.is_none());
        assert_eq!((o.res, o.scale, o.quality), ((640, 360), 2, Quality::High));
    }

    #[test]
    fn screenshot_runs_are_headless_and_options_parse() {
        let o = p(&[
            "--screenshot",
            "a.png",
            "--frames",
            "30",
            "--quality",
            "low",
            "--res",
            "320x180",
            "--shots-at",
            "2,1",
        ])
        .unwrap();
        assert!(o.headless());
        assert_eq!(
            (o.frames, o.quality, o.res),
            (Some(30), Quality::Low, (320, 180))
        );
        assert_eq!(o.shots_at, vec![1.0, 2.0]);
    }

    #[test]
    fn cache_flags_and_a_bare_level_path_parse() {
        let o = p(&["--bake", "a/x.msl", "--rebake"]).unwrap();
        assert!(o.bake && o.rebake && !o.no_cache);
        assert_eq!(o.level.as_deref(), Some(std::path::Path::new("a/x.msl")));
        assert!(!o.headless(), "--bake is handled before a run starts");
        assert!(p(&["--no-cache"]).unwrap().no_cache);
        assert!(p(&["a.msl", "b.msl"]).is_err(), "one level only");
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for args in [
            &["--quality", "ultra"][..],
            &["--scale", "0"],
            &["--res", "10"],
            &["--frames"],
            &["--bogus"],
            &["--frames", "x"],
            &["--shots-at", "nan"],
            &["--shots-at", "-1"],
            &["--shots-at", "inf"],
            &["--renderer", "vulkan"],
            &["--renderer"],
            &["--fps", "14"],
            &["--fps", "241"],
            &["--fps", "fast"],
            &["--fps"],
        ] {
            assert!(p(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn fps_takes_the_range_and_defaults_to_the_renderer() {
        assert_eq!(p(&[]).unwrap().fps, None);
        assert_eq!(p(&["--fps", "240"]).unwrap().fps, Some(240));
        assert_eq!(p(&["--fps", "15"]).unwrap().fps, Some(15));
    }

    #[test]
    fn the_renderer_is_the_gpu_in_a_window_and_software_headless_unless_asked() {
        assert_eq!(p(&[]).unwrap().renderer(), Renderer::Auto);
        assert_eq!(p(&["--bench"]).unwrap().renderer(), Renderer::Software);
        let asked = p(&["--bench", "--renderer", "hardware"]).unwrap();
        assert_eq!(asked.renderer(), Renderer::Hardware);
        let soft = p(&["--renderer", "software"]).unwrap();
        assert_eq!(soft.renderer(), Renderer::Software);
    }
}
