//! `--selftest <dir>`: a scripted run of the whole editor loop on the
//! headless xui backend. It loads the showcase, edits it, saves and reloads,
//! builds, snapshots the four views, plays 120 simulated frames with scripted
//! input, returns, edits again and snapshots after each stage. PNGs go to
//! `<dir>`; the exit code is 0 only when every check held.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use mscent_platform::{Event, Key};
use mscent_world::{Entity, Op};
use xui::Dip;
use xui::theme::Theme;
use xui::xui_canvas::snapshot::{Snapshot, Stage, render_with};

use crate::model::Document;
use crate::ui::{Editor, Msg, Probe, cmd};

mod faces;
mod keys;

/// Ticks the UI `n` times, 16 ms apart, so the render thread's frames arrive
/// and show before a snapshot.
fn pump(stage: &Stage<'_, Msg>, n: u32) {
    for _ in 0..n {
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(16));
    }
}

struct Run {
    dir: PathBuf,
    failures: Vec<String>,
}

impl Run {
    fn check(&mut self, ok: bool, what: &str) {
        println!("{} {what}", if ok { "ok  " } else { "FAIL" });
        if !ok {
            self.failures.push(what.to_string());
        }
    }

    fn shot(&mut self, stage: &Stage<'_, Msg>, name: &str) {
        let saved = stage
            .ui()
            .capture()
            .map_err(|e| format!("{e:?}"))
            .and_then(|img| {
                img.save_png(self.dir.join(name))
                    .map_err(|e| format!("{e:?}"))
            });
        self.check(saved.is_ok(), &format!("snapshot {name} {saved:?}"));
    }
}

/// Everything the script reads back from the running editor.
struct Holder {
    editor: Probe,
}

/// Runs the script; returns the process exit code.
pub fn run(dir: &Path) -> i32 {
    let light = run_theme(dir, Theme::light(), (1280.0, 800.0), script);
    let dark = run_theme(&dir.join("dark"), Theme::dark(), (1280.0, 800.0), script);
    // The layout at 2000 px wide: side panels and the build log must fit.
    let wide = run_theme(
        &dir.join("wide"),
        Theme::light(),
        (2000.0, 1124.0),
        wide_script,
    );
    light.max(dark).max(wide)
}

fn run_theme(
    dir: &Path,
    theme: Theme,
    (width, height): (f32, f32),
    script: fn(&mut Run, &Stage<'_, Msg>, &Probe),
) -> i32 {
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("cannot create {}: {e}", dir.display());
        return 2;
    }
    let run = Rc::new(RefCell::new(Run {
        dir: dir.to_path_buf(),
        failures: Vec::new(),
    }));
    let probe: Rc<RefCell<Option<Holder>>> = Rc::new(RefCell::new(None));
    let (p2, r2) = (probe.clone(), run.clone());
    let result = render_with(
        Snapshot::new(Dip(width), Dip(height)).theme(theme),
        move |ui| {
            let app = Editor::build(ui, Document::from_level(&mscent_world::showcase_level()))?;
            *p2.borrow_mut() = Some(Holder {
                editor: app.probe(),
            });
            Ok(app)
        },
        move |stage| {
            let probe = probe.borrow();
            let probe = &probe.as_ref().expect("built").editor;
            script(&mut r2.borrow_mut(), stage, probe);
        },
    );
    let mut run = run.borrow_mut();
    if let Err(e) = result {
        run.failures.push(format!("render failed: {e:?}"));
    }
    if run.failures.is_empty() {
        println!("selftest passed; images in {}", run.dir.display());
        0
    } else {
        println!("selftest FAILED: {} checks", run.failures.len());
        1
    }
}

/// Emits steps until the build ends; returns the final status message.
fn wait_for_build(stage: &Stage<'_, Msg>, probe: &Probe) -> String {
    for _ in 0..30_000 {
        stage.emit(Msg::Step);
        let m = probe.message();
        if m.starts_with("Build finished")
            || m.starts_with("Playing")
            || m.starts_with("Build failed")
            || m.starts_with("Cannot")
        {
            return m;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    "timed out waiting for the build".into()
}

/// Selects the biggest brush so a large highlight shows in every view.
fn select_biggest(probe: &Probe) {
    {
        let state = probe.state();
        let mut st = state.borrow_mut();
        let volume = |b: &&crate::model::document::BrushDef| {
            let (lo, hi) = b.bounds();
            (0..3).map(|i| hi[i] - lo[i]).product::<f64>()
        };
        let big = st
            .doc
            .brushes()
            .iter()
            .max_by(|a, b| volume(a).total_cmp(&volume(b)))
            .map(|b| b.id);
        st.selection = big.map(crate::model::Selection::Brush);
    }
}

/// The wide window: build, then the Face tool with a face framed, so the
/// properties panel, the face panel and the build log show at full width.
fn wide_script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    stage.emit(Msg::Menu(cmd::BUILD));
    let m = wait_for_build(stage, probe);
    run.check(m.starts_with("Build finished"), &format!("built: {m}"));
    select_biggest(probe);
    stage.emit(Msg::Tool(0));
    pump(stage, 20);
    run.shot(stage, "wide-01-brush.png");
    faces::frame_big_face(run, stage, probe);
    pump(stage, 30);
    run.shot(stage, "wide-02-face.png");
    faces::wide_edit(run, stage, probe);
}

fn script(run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    select_biggest(probe);
    stage.emit(Msg::Tool(0));
    run.shot(stage, "01-before-build.png");

    // Edit the showcase: a pillar and a light next to the player start.
    let start = probe
        .state()
        .borrow()
        .doc
        .entities()
        .iter()
        .find_map(|e| match e.entity {
            Entity::PlayerStart { pos, .. } => Some(pos),
            _ => None,
        });
    let Some(start) = start else {
        run.check(false, "the showcase has a player start");
        return;
    };
    let s = [f64::from(start.x), f64::from(start.y), f64::from(start.z)];
    let original = probe.state().borrow().doc.clone();
    {
        let state = probe.state();
        let mut st = state.borrow_mut();
        st.new_op = Op::Add;
        let added = st.add_box(
            [s[0] + 2.0, s[1] + 3.0, s[2] - 1.5],
            [s[0] + 3.0, s[1] + 4.0, s[2] + 2.0],
        );
        run.check(added.is_some(), "added a pillar brush");
        st.place_entity(
            crate::model::EntityKind::Light,
            [s[0] + 1.0, s[1] + 1.0, s[2] + 1.0],
        );
    }
    stage.emit(Msg::Tool(0));
    let edited = probe.state().borrow().doc.clone();
    run.check(edited != original, "the document was edited");

    // Save, then reload through the same path as File > Open.
    let file = run.dir.join("selftest.msl");
    stage.emit(Msg::SavePath(file.clone()));
    run.check(
        probe.message().starts_with("Saved"),
        &format!("saved: {}", probe.message()),
    );
    stage.emit(Msg::OpenPath(file.clone()));
    run.check(
        probe.message().starts_with("Opened"),
        &format!("opened: {}", probe.message()),
    );
    let reloaded = probe.state().borrow().doc.clone();
    run.check(
        reloaded
            .to_level()
            .ok()
            .map(|l| (l.brushes.len(), l.entities.len()))
            == edited
                .to_level()
                .ok()
                .map(|l| (l.brushes.len(), l.entities.len())),
        "the reloaded level has the same brush and entity counts",
    );
    stage.emit(Msg::OpenPath(run.dir.join("does-not-exist.msl")));
    run.check(
        probe.message().starts_with("cannot open") && probe.state().borrow().doc == reloaded,
        &format!(
            "a missing file is an error and the document stays: {}",
            probe.message()
        ),
    );

    // Build on the worker thread and look at the four views.
    stage.emit(Msg::Menu(cmd::BUILD));
    let m = wait_for_build(stage, probe);
    run.check(m.starts_with("Build finished"), &format!("built: {m}"));
    select_biggest(probe);
    stage.emit(Msg::Tool(0));
    pump(stage, 20);
    run.shot(stage, "02-four-views.png");
    let doc_before_play = probe.state().borrow().doc.clone();

    // Play: W held, a mouse turn halfway, 120 simulated frames.
    stage.emit(Msg::Menu(cmd::PLAY));
    run.check(
        probe.message().starts_with("Playing"),
        &format!("playing: {}", probe.message()),
    );
    let pane = probe.play_pane();
    pane.push_event(Event::Key {
        code: Key::Char('W'),
        pressed: true,
    });
    for i in 0..120 {
        if i == 60 {
            pane.push_event(Event::MouseMotion { dx: 90.0, dy: 0.0 });
        }
        stage.emit(Msg::Step);
        std::thread::sleep(Duration::from_millis(8));
    }
    pump(stage, 5);
    run.shot(stage, "03-in-play.png");

    // Esc returns with the document untouched.
    stage.emit(Msg::Escape);
    run.check(
        probe.state().borrow().doc == doc_before_play,
        "play left the document untouched",
    );
    run.check(
        probe.message() == "Back in the editor",
        &format!("returned: {}", probe.message()),
    );
    // Select an enemy so an entity highlight shows too.
    {
        let state = probe.state();
        let mut st = state.borrow_mut();
        let enemy = st.doc.entities().iter().find_map(|e| match e.entity {
            Entity::Enemy { .. } => Some(e.id),
            _ => None,
        });
        st.selection = enemy.map(crate::model::Selection::Entity);
    }
    stage.emit(Msg::Tool(0));
    pump(stage, 20);
    run.shot(stage, "04-after-play.png");

    // Edit again: the build goes stale and a second play works.
    {
        let state = probe.state();
        let mut st = state.borrow_mut();
        st.new_op = Op::Add;
        st.add_box(
            [s[0] - 3.0, s[1] + 3.0, s[2] - 1.5],
            [s[0] - 2.0, s[1] + 4.0, s[2] + 2.0],
        );
    }
    stage.emit(Msg::Tool(0));
    pump(stage, 20);
    run.shot(stage, "05-edit-again-stale.png");
    stage.emit(Msg::Menu(cmd::PLAY));
    let m = wait_for_build(stage, probe);
    run.check(
        m.starts_with("Playing"),
        &format!("rebuilt and playing: {m}"),
    );
    pump(stage, 30);
    run.check(
        probe.message().starts_with("Playing"),
        "second play started",
    );
    run.shot(stage, "06-second-play.png");
    stage.emit(Msg::Escape);
    pump(stage, 20);
    run.shot(stage, "07-edit-after-second-play.png");
    faces::script(run, stage, probe);
    keys::script(run, stage, probe);
}
