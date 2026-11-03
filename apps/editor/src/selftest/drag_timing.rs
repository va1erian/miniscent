//! `--ui-timing`, part two: gestures at the pace of a hand, one step per 16 ms
//! frame. A brush dragged across the top view and a stroke sculpted in the 3D
//! pane: what each step costs the UI thread, how many new pictures the 3D pane
//! showed during it, and how long after the release the picture is current and
//! then fully lit.

use std::time::{Duration, Instant};

use xui::xui_canvas::snapshot::Stage;

use super::{pump, wait_for_preview};
use crate::model::{Pointer, Selection, Tool, View};
use crate::studio::Preview;
use crate::ui::{Click, Msg, PaneDrag, Probe, ViewEvent};

/// Steps of one gesture: about a second of moving the mouse.
const STEPS: usize = 60;
/// One frame of the UI's timer.
const FRAME: Duration = Duration::from_millis(16);

/// What one gesture cost.
#[derive(Default)]
struct Costs {
    total: Duration,
    worst: Duration,
    /// Steps after which the 3D pane showed another picture.
    pictures: usize,
    last: Option<(u32, u32, u64)>,
}

impl Costs {
    /// Runs one step at the pace of a frame and records it.
    fn step(&mut self, stage: &Stage<'_, Msg>, probe: &Probe, work: impl FnOnce()) {
        let t = Instant::now();
        work();
        stage.emit(Msg::Step);
        let _ = stage.ui().capture();
        let cost = t.elapsed();
        (self.worst, self.total) = (self.worst.max(cost), self.total + cost);
        let shown = probe.view_pane().frame_checksum();
        self.pictures += usize::from(shown != self.last);
        self.last = shown;
        std::thread::sleep(FRAME.saturating_sub(cost));
    }

    /// Waits for the picture after the release and prints the line.
    fn report(&self, stage: &Stage<'_, Msg>, probe: &Probe, what: &str) {
        let released = Instant::now();
        while probe.preview() == Preview::Updating && released.elapsed() < Duration::from_secs(60) {
            stage.emit(Msg::Step);
            std::thread::sleep(Duration::from_millis(1));
        }
        let current = released.elapsed();
        let end = wait_for_preview(stage, probe);
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        println!(
            "timing {what:<26} step mean {:.3} ms worst {:.3} ms; {} new 3D pictures in {STEPS} \
             steps; current {:.0} ms after release, {end:?} after {:.0} ms",
            ms(self.total) / STEPS as f64,
            ms(self.worst),
            self.pictures,
            ms(current),
            ms(released.elapsed()),
        );
    }
}

/// The centre of the brush that is `which` in the document, in the top view.
fn centre(probe: &Probe, which: usize) -> Option<(crate::model::document::Id, [f64; 2])> {
    let state = probe.state();
    let st = state.borrow();
    let b = st.doc.brushes().get(which)?;
    let (lo, hi) = b.bounds();
    let (a, c) = View::Top.axes();
    Some((b.id, [(lo[a] + hi[a]) * 0.5, (lo[c] + hi[c]) * 0.5]))
}

/// Drags brush `which` along the top view's first axis and prints the costs.
pub(super) fn drag(stage: &Stage<'_, Msg>, probe: &Probe, label: &str, which: usize) {
    wait_for_preview(stage, probe);
    pump(stage, 10);
    let Some((id, from)) = centre(probe, which) else {
        println!("timing drag {label}: no brush {which}");
        return;
    };
    let state = probe.state();
    {
        let mut st = state.borrow_mut();
        st.set_tool(Tool::Select);
        st.selection = Some(Selection::Brush(id));
        // Every step moves the brush, as a hand on a fine grid does.
        st.grid = 0.125;
    }
    let at = |i: usize| Pointer {
        view: View::Top,
        pos: [from[0] + i as f64 * 0.125, from[1]],
        px: 0.02,
    };
    state.borrow_mut().pointer_down(at(0));
    stage.emit(Msg::View(ViewEvent::Changed));
    let mut costs = Costs::default();
    for i in 1..=STEPS {
        costs.step(stage, probe, || {
            state.borrow_mut().pointer_move(at(i));
            stage.emit(Msg::View(ViewEvent::Changed));
        });
    }
    state.borrow_mut().pointer_up(at(STEPS));
    stage.emit(Msg::View(ViewEvent::Changed));
    costs.report(stage, probe, &format!("drag {label}"));
}

/// Sculpts one stroke across the middle of the 3D pane.
pub(super) fn sculpt(stage: &Stage<'_, Msg>, probe: &Probe, label: &str) {
    wait_for_preview(stage, probe);
    pump(stage, 10);
    probe.state().borrow_mut().set_tool(Tool::Voxel);
    let pane = probe.view_pane();
    let at = |i: usize, released: bool| PaneDrag {
        nx: 0.35 + i as f32 * 0.005,
        ny: 0.5,
        aspect: 1.6,
        released,
    };
    pane.push_click(Click {
        nx: 0.35,
        ny: 0.5,
        aspect: 1.6,
        right: false,
        ctrl: false,
        alt: false,
    });
    let mut costs = Costs::default();
    for i in 1..=STEPS {
        costs.step(stage, probe, || pane.push_drag(at(i, false)));
    }
    pane.push_drag(at(STEPS, true));
    stage.emit(Msg::Step);
    costs.report(stage, probe, &format!("sculpt {label}"));
    probe.state().borrow_mut().set_tool(Tool::Select);
}
