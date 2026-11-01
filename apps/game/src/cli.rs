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
  --autoclose-ms <n>       desktop: quit after n milliseconds
  --help
";

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
    pub autoclose_ms: Option<u64>,
    pub help: bool,
}

impl Options {
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

/// Parses the arguments after the program name.
pub fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Options, String> {
    let mut o = Options {
        level: None,
        corridor: false,
        quality: Quality::High,
        scale: 2,
        res: (640, 360),
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
        autoclose_ms: None,
        help: false,
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
            "--autoclose-ms" => {
                o.autoclose_ms = Some(
                    value("--autoclose-ms")?
                        .parse()
                        .map_err(|_| "--autoclose-ms must be a number")?,
                );
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
        ] {
            assert!(p(args).is_err(), "{args:?}");
        }
    }
}
