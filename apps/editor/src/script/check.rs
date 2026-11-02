//! Checks the script against the level as it is typed, off the UI thread, the
//! way `--check` does: compile, then run its top level on a real game. Only the
//! newest text is checked, one job at a time, once the typing pauses.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::{Duration, Instant};

use miniscent_game::scripts;
use mscent_game::{Arena, Game};
use mscent_math::{Vec3, Vec3d};
use mscent_script::{ScriptError, Source};
use mscent_world::{Brush, Entity, Level, Op};

use super::scan::{Define, defines};
use crate::model::Document;

/// How long the text must stay put before it is checked.
const SETTLE: Duration = Duration::from_millis(300);

/// Name of the script of a level that has no file yet.
const UNSAVED: &str = "script.st";

/// One thing to tell the author.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    /// The zero-based line in the level's own script. `None` when the trouble is
    /// elsewhere: a library, or the level.
    pub line: Option<usize>,
    pub text: String,
    pub warning: bool,
}

/// What a check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub problems: Vec<Problem>,
    /// The library's functions, for completion.
    pub library: Vec<Define>,
}

fn problem(e: &ScriptError, script: &str, warning: bool) -> Problem {
    let own = &*e.file == script && e.line > 0;
    Problem {
        line: own.then(|| e.line as usize - 1),
        text: if own {
            format!("Line {}: {}", e.line, e.msg)
        } else {
            e.to_string()
        },
        warning,
    }
}

/// A level with no start or no brushes cannot run, but its script can still be
/// checked: give the check what the level does not have yet.
fn runnable(mut level: Level) -> Level {
    if level.static_brushes().is_empty() {
        let (lo, hi) = (Vec3d::new(-1.0, -1.0, -1.0), Vec3d::new(1.0, 1.0, 1.0));
        if let Ok(room) = Brush::cuboid(lo, hi, Op::Subtract, 0) {
            level.brushes.push(room);
        }
    }
    let has_start = |e: &Entity| matches!(e, Entity::PlayerStart { .. });
    if !level.entities.iter().any(has_start) {
        level.entities.push(Entity::PlayerStart {
            pos: Vec3::new(0.0, 0.0, 0.0),
            yaw: 0.0,
        });
    }
    level
}

/// Runs the checks. Never panics on any text, and costs what `--check` costs.
pub fn check(level: Level, level_path: Option<&Path>, text: &str) -> Report {
    let files = scripts::library_files(level_path);
    let library = files.iter().flat_map(|(_, t)| defines(t)).collect();
    if text.trim().is_empty() {
        return Report {
            problems: Vec::new(),
            library,
        };
    }
    let script = level_path
        .and_then(|p| {
            p.with_extension("st")
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| UNSAVED.to_string());
    let mut sources: Vec<Source> = files
        .iter()
        .map(|(name, text)| Source { name, text })
        .collect();
    sources.push(Source {
        name: &script,
        text,
    });
    let problems = match Arena::from_scripted_level(&runnable(level)) {
        Ok(arena) => run(arena, &sources, &script),
        Err(why) => vec![Problem {
            line: None,
            text: format!("The level does not build yet ({why:?}), so nothing was checked"),
            warning: true,
        }],
    };
    Report { problems, library }
}

fn run(arena: Arena, sources: &[Source], script: &str) -> Vec<Problem> {
    let mut game = Game::new(arena);
    match game.load_script(sources) {
        Err(errors) => errors.iter().map(|e| problem(e, script, false)).collect(),
        Ok(warnings) => {
            let failed = game.take_script_errors();
            let bad = failed.iter().map(|e| problem(e, script, false));
            let soft = warnings.iter().map(|e| problem(e, script, true));
            bad.chain(soft).collect()
        }
    }
}

/// The checker the UI tick drives.
pub struct Checker {
    /// `None` comes back when the level has an invalid brush, which the status
    /// bar already reports, so what was known stands.
    job: Option<Receiver<Option<Report>>>,
    /// The level the library in the report was read for.
    library_for: Option<Option<PathBuf>>,
    /// What the job running, or the last one, was given.
    checked: Option<(String, Document)>,
    /// What was last seen, and when it changed.
    seen: Option<(String, Document)>,
    changed_at: Instant,
    report: Report,
}

impl Default for Checker {
    fn default() -> Self {
        Checker {
            job: None,
            library_for: None,
            checked: None,
            seen: None,
            changed_at: Instant::now(),
            report: Report::default(),
        }
    }
}

impl Checker {
    pub fn report(&self) -> &Report {
        &self.report
    }

    /// Whether the report is about this text and level, with nothing on the way.
    #[cfg(test)]
    pub fn is_current(&self, text: &str, doc: &Document) -> bool {
        self.job.is_none()
            && self
                .checked
                .as_ref()
                .is_some_and(|(t, d)| t == text && d == doc)
    }

    /// One tick. Returns whether the report changed. Never blocks.
    pub fn tick(&mut self, text: &str, doc: &Document) -> bool {
        let mut changed = self.collect();
        let same =
            |x: &Option<(String, Document)>| x.as_ref().is_some_and(|(t, d)| t == text && d == doc);
        if !same(&self.seen) {
            self.seen = Some((text.to_string(), doc.clone()));
            self.changed_at = Instant::now();
        }
        let first = self.checked.is_none();
        let settled = first || self.changed_at.elapsed() >= SETTLE;
        if self.job.is_none() && settled && !same(&self.checked) {
            changed |= self.start(text, doc);
        }
        changed
    }

    fn collect(&mut self) -> bool {
        let Some(rx) = &self.job else {
            return false;
        };
        let report = match rx.try_recv() {
            Ok(Some(report)) => report,
            Ok(None) => {
                self.job = None;
                return false;
            }
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Report {
                problems: vec![Problem {
                    line: None,
                    text: "The script check crashed; this script may do the same in the game"
                        .to_string(),
                    warning: false,
                }],
                library: self.report.library.clone(),
            },
        };
        self.job = None;
        let changed = report != self.report;
        self.report = report;
        changed
    }

    /// Starts a job, unless there is nothing to do: with no script and the
    /// library already read, an edit to the level changes nothing here. Whether
    /// the report changed by itself.
    fn start(&mut self, text: &str, doc: &Document) -> bool {
        self.checked = Some((text.to_string(), doc.clone()));
        let path = doc.level_path().map(Path::to_path_buf);
        if text.trim().is_empty() && self.library_for.as_ref() == Some(&path) {
            let had_problems = !self.report.problems.is_empty();
            self.report.problems.clear();
            return had_problems;
        }
        self.library_for = Some(path.clone());
        let (tx, rx) = channel();
        let (text, doc) = (text.to_string(), doc.clone());
        let started = std::thread::Builder::new()
            .name("mscent-script-check".into())
            .spawn(move || {
                let report = doc.to_level().ok();
                let _ = tx.send(report.map(|level| check(level, path.as_deref(), &text)));
            });
        if started.is_ok() {
            self.job = Some(rx);
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{EditorState, EntityKind, Selection};

    fn level() -> Level {
        Document::new().to_level().expect("an empty level")
    }

    #[test]
    fn a_script_with_no_problems_reports_none_even_with_an_empty_level() {
        let report = check(level(), None, "say: 'hello'.\n");
        assert_eq!(report.problems, []);
    }

    #[test]
    fn a_syntax_error_has_its_line() {
        let report = check(level(), None, "say: 'a'.\nsay: 'b\n");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert_eq!(report.problems[0].line, Some(1));
        assert!(!report.problems[0].warning);
    }

    #[test]
    fn a_typo_in_a_message_or_a_name_is_found_where_it_is() {
        let report = check(level(), None, "\nsaay: 'a'.\n");
        assert_eq!(report.problems[0].line, Some(1), "{:?}", report.problems);
        assert!(report.problems[0].text.starts_with("Line 2:"));
        let report = check(level(), None, "\n\n(mover: 'nope') moving.\n");
        assert_eq!(report.problems[0].line, Some(2), "{:?}", report.problems);
    }

    #[test]
    fn a_top_level_that_fails_when_it_runs_is_reported_too() {
        // The name is built at run time, so only running it can tell.
        let report = check(level(), None, "mover: 'no' , 'pe'.\n");
        assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
        assert!(
            report.problems[0].text.contains("nope"),
            "{:?}",
            report.problems
        );
    }

    #[test]
    fn a_name_in_the_level_is_found_and_taking_it_away_is_a_problem() {
        let mut st = EditorState::new(Document::new());
        let id = st.place_entity(EntityKind::Marker, [1.0, 1.0, 1.0]);
        st.selection = Some(Selection::Entity(id));
        st.sync_draft();
        st.draft.name = "spot".into();
        assert!(st.apply_draft());
        let text = "spawn: 'crate' at: (marker: 'spot') tag: 't'.\n";
        let ok = check(st.doc.to_level().unwrap(), None, text);
        assert_eq!(ok.problems, []);
        assert!(st.undo());
        let gone = check(st.doc.to_level().unwrap(), None, text);
        assert_eq!(gone.problems[0].line, Some(0), "{:?}", gone.problems);
    }

    #[test]
    fn the_library_is_read_for_completion() {
        let report = check(level(), None, "");
        assert!(
            report
                .library
                .iter()
                .any(|d| d.selector == "autoDoor:travel:key:")
        );
        assert!(report.library.iter().any(|d| d.selector == "wave:kind:"));
    }

    #[test]
    fn hostile_scripts_come_back_as_problems() {
        for text in [
            "[true] whileTrue: [ ].",
            "x := 'a'. [true] whileTrue: [x := x , x].",
            &"(".repeat(5000),
            &"[".repeat(5000),
            "\"",
            "'",
        ] {
            let _ = check(level(), None, text);
        }
    }

    #[test]
    fn with_no_script_a_level_edit_starts_no_job_and_old_problems_go() {
        let mut c = Checker::default();
        let doc = Document::new();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !c.is_current("saay: 1.", &doc) && Instant::now() < deadline {
            c.tick("saay: 1.", &doc);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.report().problems.len(), 1);
        c.start("", &doc);
        assert!(c.job.is_none(), "the library is already read");
        assert_eq!(c.report().problems, []);
        assert!(!c.report().library.is_empty(), "and completion keeps it");
    }

    #[test]
    fn the_checker_settles_on_the_newest_text_without_blocking() {
        let doc = Document::new();
        let mut c = Checker::default();
        assert!(!c.is_current("saay: 1.", &doc));
        let deadline = Instant::now() + Duration::from_secs(30);
        while !c.is_current("saay: 1.", &doc) && Instant::now() < deadline {
            c.tick("saay: 1.", &doc);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.report().problems.len(), 1);
        while !c.is_current("say: 'a'.", &doc) && Instant::now() < deadline {
            c.tick("say: 'a'.", &doc);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(c.report().problems, []);
    }
}
