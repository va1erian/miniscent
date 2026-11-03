//! `--ui-timing`: how long the UI thread spends on what it does all the time, on
//! the showcase. The numbers are for comparing two builds on one machine.

use std::time::{Duration, Instant};

use xui::xui_canvas::snapshot::Stage;

use super::{Run, pump, wait_for_preview};
use crate::ui::script_pane::ScriptMsg;
use crate::ui::{Msg, Probe, cmd};

/// Rounds of each thing measured.
const ROUNDS: usize = 300;

/// The mean and worst time of `work`, in milliseconds.
fn measure(mut work: impl FnMut(usize)) -> (f64, f64) {
    let mut times: Vec<Duration> = Vec::with_capacity(ROUNDS);
    for i in 0..ROUNDS {
        let start = Instant::now();
        work(i);
        times.push(start.elapsed());
    }
    let total: Duration = times.iter().sum();
    let worst = times.iter().max().copied().unwrap_or_default();
    (
        total.as_secs_f64() * 1000.0 / ROUNDS as f64,
        worst.as_secs_f64() * 1000.0,
    )
}

fn report(what: &str, (mean, worst): (f64, f64)) {
    println!("timing {what:<28} mean {mean:8.3} ms   worst {worst:8.3} ms");
}

/// The re-flow alone, for each way the pane can be.
fn pane_states(stage: &Stage<'_, Msg>, probe: &Probe) {
    let reflow = || {
        measure(|_| {
            stage.ui().invalidate_layout();
            stage.emit(Msg::Step);
        })
    };
    let picture = || {
        measure(|_| {
            let _ = stage.ui().capture();
        })
    };
    stage.emit(Msg::Menu(cmd::SCRIPT_PANE));
    pump(stage, 5);
    report("a re-flow, pane open", reflow());
    report("a picture, pane open", picture());
    stage.emit(Msg::Menu(cmd::GUIDE));
    pump(stage, 5);
    super::guide::wait_for_guide(stage, probe);
    report("a re-flow, guide open", reflow());
    report("a picture, guide open", picture());
    stage.emit(Msg::Script(ScriptMsg::Topic(7)));
    pump(stage, 5);
    super::guide::wait_for_guide(stage, probe);
    report("a re-flow, reference page", reflow());
    report("a picture, reference page", picture());
    stage.emit(Msg::Script(ScriptMsg::Float(true)));
    pump(stage, 5);
    report("a re-flow, own window", reflow());
    stage.emit(Msg::Script(ScriptMsg::Float(false)));
    stage.emit(Msg::Menu(cmd::SCRIPT_PANE));
    pump(stage, 5);
    report("a re-flow, pane closed again", reflow());
    report("a picture, pane closed again", picture());
}

pub(super) fn script(_run: &mut Run, stage: &Stage<'_, Msg>, probe: &Probe) {
    wait_for_preview(stage, probe);
    pump(stage, 20);
    report("a tick", measure(|_| stage.emit(Msg::Step)));
    report(
        "the pointer moving",
        measure(|i| {
            stage.hover(300 + (i as i32 % 200), 130);
        }),
    );
    report(
        "a re-flow",
        measure(|_| {
            stage.ui().invalidate_layout();
            stage.emit(Msg::Step);
        }),
    );
    report(
        "a picture of the window",
        measure(|_| {
            let _ = stage.ui().capture();
        }),
    );
    report("a new 3D frame, painted", new_frame(stage, probe));
    pane_states(stage, probe);
    drags(stage, probe);
}

/// A small brush and the biggest one on the showcase, then a voxel level.
fn drags(stage: &Stage<'_, Msg>, probe: &Probe) {
    super::drag_timing::drag(stage, probe, "showcase, brush 5", 5);
    super::drag_timing::drag(stage, probe, "showcase, brush 0", 0);
    let cave =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/levels/voxel-cave.msl");
    stage.emit(Msg::OpenPath(cave));
    for which in 0..2 {
        super::drag_timing::drag(stage, probe, &format!("voxel-cave, brush {which}"), which);
    }
    super::drag_timing::sculpt(stage, probe, "voxel-cave");
}

/// What the UI thread pays for each picture the render thread sends: a frame the
/// size of the pane through the pane's own path, then a picture of the window.
fn new_frame(stage: &Stage<'_, Msg>, probe: &Probe) -> (f64, f64) {
    let pane = probe.view_pane();
    let (w, h) = pane.size();
    let rgba = |i: usize| vec![(i % 256) as u8; w as usize * h as usize * 4];
    let frames = [rgba(40), rgba(200)];
    measure(|i| {
        pane.set_rgba(w, h, frames[i % 2].clone());
        let _ = stage.ui().capture();
    })
}
