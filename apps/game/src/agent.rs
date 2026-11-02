//! The commands that let an agent iterate on a level with no rebuild:
//! `--check`, `--test`, `--script-api`, `--dump-level` and `--pack-level`.
//! Each fails with an error, so the process exit code tells the result.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use mscent_collide::STEP_HZ;
use mscent_game::{Arena, Game, Thing, natives, test_natives};
use mscent_render::png_encode::encode_png;
use mscent_render::{BakeCache, LevelRenderer};
use mscent_script::{ScriptError, Source, language_selectors};
use mscent_world::{Level, format, level_text};

use crate::assets;
use crate::scripts::{self, ScriptFiles};
use crate::session::{GameSession, SessionConfig};
use crate::trace::Trace;

/// A test that has not finished after this many game seconds has hung.
const TEST_LIMIT_SECONDS: u64 = 3600;
/// Size of a test's screenshots.
const SHOT_SIZE: (u32, u32) = (640, 360);

const LANGUAGE: &str = "\
The language, in full:
  x := 3.                        assignment; the first one declares the variable
  door moveTo: 'open' in: 1.5.   keyword message: receiver, then name: arg name: arg
  say: 'hello'.                  no receiver: a global function
  player hp.                     unary message; a bare word that is no variable is a function
  a + b * c.                     binary runs strictly left to right: (a + b) * c
  'n = ' , 3.                    comma concatenates and converts to text
  {1. 2. 3}.                     array, 1-indexed: a at: 1, a size, a add: x
  [:x | x + 1].                  block; call it with b value: 1; it can change outer variables
  c ifTrue: [ ] ifFalse: [ ].    also ifTrue:, ifFalse:, and: [ ], or: [ ], not, isNil
  [c] whileTrue: [ ].            also whileFalse:, n timesRepeat: [ ], a do: [:x | ]
  define name: a other: b [ ^ a + b ]   function; it sees top-level variables
  define arr first [ ^ arr at: 1 ]      with a receiver: the first word names it
  wait: 2.                       pause this handler only
  fork: [ ].                     run a block alongside
  \"text\"                         a comment; strings use single quotes
A handler that takes a parameter is given what its event was about.
A statement ends with a period. ^ outside a define ends that handler.";

/// `--script-api`: everything a script can call, from the table that registers it.
pub fn script_api() -> String {
    let list = |names: Vec<&str>| {
        let mut names = names;
        names.sort_unstable();
        names.iter().map(|n| format!("  {n}\n")).collect::<String>()
    };
    let game: Vec<&str> = natives().iter().map(|n| n.0).collect();
    let test: Vec<&str> = test_natives().iter().map(|n| n.0).collect();
    format!(
        "{LANGUAGE}\n\nGame functions:\n{}\nOnly in a test script (--test):\n{}\nProvided by the language:\n{}",
        list(game),
        list(test),
        list(language_selectors()),
    )
}

/// `--dump-level`: the level as text.
pub fn dump_level(path: &Path) -> Result<String, String> {
    Ok(level_text::to_text(&assets::load_level(path)?))
}

/// `--pack-level`: reads text and writes the binary level. Anything refused is
/// reported with its line, and nothing is written.
pub fn pack_level(from: &Path, to: &Path) -> Result<String, String> {
    let cannot_read = |e| format!("cannot read {}: {e}", from.display());
    let len = std::fs::metadata(from).map_err(cannot_read)?.len();
    if len > format::MAX_FILE_BYTES as u64 {
        return Err(format!(
            "{} is {len} bytes; the limit is {}",
            from.display(),
            format::MAX_FILE_BYTES
        ));
    }
    let text = std::fs::read_to_string(from).map_err(cannot_read)?;
    let level = level_text::from_text(&text).map_err(|e| format!("{}: {e}", from.display()))?;
    let bytes = format::write(&level);
    std::fs::write(to, &bytes).map_err(|e| format!("cannot write {}: {e}", to.display()))?;
    Ok(format!("wrote {} ({} bytes)", to.display(), bytes.len()))
}

fn error_list(what: &str, errors: &[ScriptError]) -> String {
    let lines: Vec<String> = errors.iter().map(ToString::to_string).collect();
    format!("{what}:\n{}", lines.join("\n"))
}

fn sources_of(files: Option<&ScriptFiles>) -> Vec<Source<'_>> {
    files.map(ScriptFiles::sources).unwrap_or_default()
}

fn kind_name(thing: Thing) -> &'static str {
    match thing {
        Thing::Mover(_) => "mover",
        Thing::Zone(_) => "zone",
        Thing::Marker(_) => "marker",
        Thing::Enemy(_) => "enemy",
    }
}

/// The script's string literals. Comments are skipped: an apostrophe in one is
/// not the start of a string.
fn quoted_strings(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find(['\'', '"']) {
        let quote = char::from(rest.as_bytes()[open]);
        let inner = &rest[open + 1..];
        let Some(close) = inner.find(quote) else {
            break;
        };
        if quote == '\'' {
            found.push(&inner[..close]);
        }
        rest = &inner[close + 1..];
    }
    found
}

/// Every named thing and tag, and whether any script text mentions it. A name
/// built at run time counts as unused here, so this is a warning, not an error.
fn inventory(game: &Game, text: &[Source]) -> (String, usize) {
    let quoted: Vec<&str> = text.iter().flat_map(|s| quoted_strings(s.text)).collect();
    // A literal may be the start of a name, as `markers:` allows.
    let used = |name: &str| quoted.iter().any(|q| !q.is_empty() && name.starts_with(q));
    let (mut out, mut unused) = (String::new(), 0);
    let mut row = |kind: &str, name: &str| {
        let ok = used(name);
        unused += usize::from(!ok);
        let note = if ok { "" } else { "  (no script names it)" };
        out += &format!("  {kind:<7} {name}{note}\n");
    };
    let names = &game.arena.names;
    names
        .by_name
        .iter()
        .for_each(|(n, t)| row(kind_name(*t), n));
    names.tagged.keys().for_each(|n| row("tag", n));
    (out, unused)
}

/// `--check`: compiles the level's script and runs its top level against the
/// level, without opening a window. Warnings do not fail it.
pub fn check(level_path: &Path) -> Result<String, String> {
    let level = assets::load_level(level_path)?;
    let files = scripts::for_level(level_path)?;
    let arena = Arena::from_scripted_level(&level).map_err(|e| format!("{e:?}"))?;
    let mut game = Game::new(arena);
    let sources = sources_of(files.as_ref());
    if files.is_none() {
        let (rows, _) = inventory(&game, &[]);
        return Ok(format!("no script beside this level\n{rows}"));
    }
    let warnings = game
        .load_script(&sources)
        .map_err(|e| error_list("the script did not load", &e))?;
    let errors = game.take_script_errors();
    if !errors.is_empty() {
        return Err(error_list("the script's top level failed", &errors));
    }
    let (rows, unused) = inventory(&game, &sources);
    let mut out = String::new();
    for w in &warnings {
        out += &format!("warning: {w}\n");
    }
    if unused > 0 {
        out += &format!("warning: {unused} name(s) in the level no script mentions\n");
    }
    out += &format!(
        "ok: {} source file(s), {} warning(s)\n{rows}",
        sources.len(),
        warnings.len()
    );
    Ok(out)
}

/// Where a `--test` run writes.
pub struct TestOutput {
    pub trace: Option<PathBuf>,
    pub shots_dir: PathBuf,
    pub assets: Option<PathBuf>,
}

fn take_shots(
    game: &mut Game,
    level: &Level,
    out: &TestOutput,
    pending: &mut Shooter,
) -> Result<(), String> {
    for name in game.take_shots() {
        pending.write(game, level, out, &name)?;
    }
    Ok(())
}

/// Renders only for a test that asks: a test that takes no pictures needs no
/// textures or lightmap, and runs far faster for it.
#[derive(Default)]
struct Shooter {
    renderer: Option<LevelRenderer>,
}

impl Shooter {
    fn write(
        &mut self,
        game: &mut Game,
        level: &Level,
        out: &TestOutput,
        name: &str,
    ) -> Result<(), String> {
        let cfg = SessionConfig::default();
        let renderer = match self.renderer.take() {
            Some(r) => r,
            None => {
                let materials = assets::load_materials(out.assets.as_deref())?;
                LevelRenderer::from_level_cached(
                    level,
                    materials,
                    &cfg.build,
                    &mut BakeCache::new(),
                )
                .map_err(|e| format!("cannot build the renderer: {e:?}"))?
            }
        };
        let dummy = game.clone();
        let mut session = GameSession::with_game(std::mem::replace(game, dummy), renderer, &cfg);
        let frame = session
            .render(SHOT_SIZE.0, SHOT_SIZE.1)
            .ok_or("cannot render a frame")?;
        let png =
            encode_png(SHOT_SIZE.0, SHOT_SIZE.1, frame.pixels()).ok_or("cannot encode a shot")?;
        std::fs::create_dir_all(&out.shots_dir).map_err(|e| e.to_string())?;
        let path = out.shots_dir.join(name);
        std::fs::write(&path, png).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        let (back, renderer) = session.into_parts();
        *game = back;
        self.renderer = Some(renderer);
        Ok(())
    }
}

/// A file's bytes, or nowhere, so a run with no `--trace` writes nothing.
fn trace_writer(path: Option<&Path>) -> Result<Box<dyn Write>, String> {
    match path {
        Some(p) => {
            if let Some(dir) = p.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            let file = File::create(p).map_err(|e| format!("cannot write {}: {e}", p.display()))?;
            Ok(Box::new(BufWriter::new(file)))
        }
        None => Ok(Box::new(std::io::sink())),
    }
}

/// What a test run found.
#[derive(Default)]
struct Tally {
    passed: usize,
    failed: Vec<String>,
    errors: Vec<String>,
}

fn collect<W: Write>(
    game: &mut Game,
    trace: &mut Trace<W>,
    tally: &mut Tally,
) -> Result<(), String> {
    let io = |e: std::io::Error| format!("cannot write the trace: {e}");
    for c in game.take_checks() {
        trace.check(c.passed, &c.because).map_err(io)?;
        if c.passed {
            tally.passed += 1;
        } else {
            tally.failed.push(c.because);
        }
    }
    for e in game.take_script_errors() {
        let text = e.to_string();
        trace.error(&text).map_err(io)?;
        tally.errors.push(text);
    }
    Ok(())
}

/// `--test`: plays the level with no window, as fast as the machine allows,
/// until the test script ends. The test is one more task, so its `wait:` is what
/// advances the game.
pub fn run_test(level_path: &Path, test_path: &Path, out: &TestOutput) -> Result<String, String> {
    let level = assets::load_level(level_path)?;
    let files = scripts::for_test(level_path, test_path)?;
    let arena = Arena::from_scripted_level(&level).map_err(|e| format!("{e:?}"))?;
    let mut game = Game::new(arena);
    let sources = files.sources();
    game.load_test_script(&sources)
        .map_err(|e| error_list("the script did not load", &e))?;
    let mut trace = Trace::new(trace_writer(out.trace.as_deref())?);
    let (mut tally, mut shooter) = (Tally::default(), Shooter::default());
    let io = |e: std::io::Error| format!("cannot write the trace: {e}");
    let limit = TEST_LIMIT_SECONDS * STEP_HZ;
    let mut steps = 0;
    collect(&mut game, &mut trace, &mut tally)?;
    take_shots(&mut game, &level, out, &mut shooter)?;
    while !game.test_finished() && steps < limit {
        let input = game.test_input();
        game.step(&input);
        steps += 1;
        trace.record_step(&mut game).map_err(io)?;
        collect(&mut game, &mut trace, &mut tally)?;
        take_shots(&mut game, &level, out, &mut shooter)?;
    }
    trace.flush().map_err(io)?;
    report(&tally, steps, steps >= limit)
}

fn report(tally: &Tally, steps: u64, hung: bool) -> Result<String, String> {
    let mut text = String::new();
    for f in &tally.failed {
        text += &format!("FAILED: {f}\n");
    }
    for e in &tally.errors {
        text += &format!("ERROR: {e}\n");
    }
    if hung {
        text += &format!("HUNG: still running after {TEST_LIMIT_SECONDS} game seconds\n");
    }
    let seconds = steps as f64 / STEP_HZ as f64;
    text += &format!(
        "{} passed, {} failed, {} script error(s), {seconds:.2} game seconds",
        tally.passed,
        tally.failed.len(),
        tally.errors.len()
    );
    if tally.failed.is_empty() && tally.errors.is_empty() && !hung {
        Ok(text)
    } else {
        Err(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_level_refuses_a_file_past_the_limit_without_reading_it() {
        let dir = std::env::temp_dir().join(format!("mscent-pack-big-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.txt");
        let file = File::create(&big).unwrap();
        file.set_len(format::MAX_FILE_BYTES as u64 + 1).unwrap();
        drop(file);
        let why = pack_level(&big, &dir.join("out.msl")).unwrap_err();
        assert!(why.contains("bytes; the limit is"), "{why}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
