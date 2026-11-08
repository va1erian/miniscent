//! `--live-timing`: scripted gestures in the real window, on whichever backend
//! draws it, with the frame meter on. A thread sends a step every
//! [`STEP`], as a mouse reports moves (skipping steps the UI is not ready for); each phase then prints how often the 2D
//! views and the 3D pane really painted, the longest gap between two paints and
//! what each step and paint cost. The editor quits after the last phase.
//!
//! The goal it checks: 60 paints a second while editing, with no gap over two
//! frames.

use std::time::{Duration, Instant};

use super::meter::{self, Lane};
use super::pace::{FRAME, Pacer};
use super::view3d::{Click, PaneDrag};
use super::{Editor, Msg};
use crate::model::{Pointer, Selection, Tool, View};
use crate::studio::Preview;

/// How often a step comes: a mouse at 125 Hz.
pub const STEP: Duration = Duration::from_millis(8);
/// How long each gesture lasts.
const PHASE: Duration = Duration::from_secs(3);
/// The longest wait for the first picture of the level.
const SETTLE: Duration = Duration::from_secs(60);

/// One part of the script.
#[derive(Clone, Copy, Debug)]
enum Phase {
    /// Waits for the level's first lit picture; reports nothing.
    Settle,
    Idle,
    /// Drags brush `n` back and forth in the top view.
    Drag(usize),
    /// Sculpts a stroke across the 3D pane.
    Sculpt,
    /// Flies the 3D camera forward.
    Fly,
}

const SCRIPT: [Phase; 7] = [
    Phase::Settle,
    Phase::Idle,
    Phase::Drag(0),
    Phase::Drag(1),
    Phase::Sculpt,
    Phase::Fly,
    Phase::Idle,
];

/// What paces the editor in a real window: the frame tick, and while it runs,
/// `--live-timing`'s script. Both stay empty in scripted runs, which step by hand.
#[derive(Default)]
pub struct Clock {
    pub live: Option<LiveRun>,
    pub ticker: Option<Pacer>,
}

/// Where the script is.
pub struct LiveRun {
    at: usize,
    began: Option<Instant>,
    steps: usize,
    /// The dragged brush's centre in the top view, once its drag began.
    from: [f64; 2],
    /// Where the 3D camera was when the flight began.
    eye: [f64; 3],
    /// Sends the steps.
    pacer: Pacer,
}

impl LiveRun {
    fn new(pacer: Pacer) -> LiveRun {
        LiveRun {
            pacer,
            at: 0,
            began: None,
            steps: 0,
            from: [0.0; 2],
            eye: [0.0; 3],
        }
    }
}

/// A triangle wave in -1..=1 with a period of `steps`, so a gesture goes back
/// and forth and stays in view.
fn sweep(i: usize, steps: usize) -> f64 {
    let t = (i % steps) as f64 / steps as f64;
    if t < 0.5 {
        4.0 * t - 1.0
    } else {
        3.0 - 4.0 * t
    }
}

impl Editor {
    /// Turns the meter on and starts the script and its pacing thread.
    pub fn start_live(&mut self, ui: &xui::app::Ui<Msg>) {
        meter::set_on(true);
        self.clock.live = Some(LiveRun::new(Pacer::start(ui.proxy(), Msg::Live, STEP)));
    }

    /// Starts the frame tick, 60 times a second, for a real window. Scripted
    /// runs send `Msg::Step` themselves instead.
    pub fn start_ticks(&mut self, ui: &xui::app::Ui<Msg>) {
        self.clock.ticker = Some(Pacer::start(ui.proxy(), Msg::Tick, FRAME));
    }

    /// A step of the script; the editor quits after the last.
    pub(super) fn live_message(&mut self, ui: &xui::app::Ui<Msg>) {
        if !self.live_step() && self.clock.live.is_some() {
            self.clock.live = None;
            ui.quit();
        }
    }

    /// One step of `--live-timing`. False once the script is over.
    pub(super) fn live_step(&mut self) -> bool {
        let Some(mut run) = self.clock.live.take() else {
            return false;
        };
        run.pacer.arrived();
        let Some(&phase) = SCRIPT.get(run.at) else {
            return false;
        };
        let began = match run.began {
            Some(t) => t,
            None => {
                self.live_begin(phase, &mut run);
                *run.began.insert(Instant::now())
            }
        };
        let done = match phase {
            Phase::Settle => {
                !matches!(self.preview.get(), Preview::Updating) || began.elapsed() > SETTLE
            }
            _ => began.elapsed() >= PHASE,
        };
        if done {
            self.live_end(phase, &run);
            let notes = meter::take();
            if !matches!(phase, Phase::Settle) {
                println!(
                    "{}",
                    meter::report(&format!("{phase:?}"), &notes, began.elapsed())
                );
            }
            (run.at, run.began, run.steps) = (run.at + 1, None, 0);
        } else {
            run.steps += 1;
            meter::time(Lane::Step, || self.live_gesture(phase, &run));
        }
        let more = run.at < SCRIPT.len();
        self.clock.live = Some(run);
        more
    }

    fn live_begin(&mut self, phase: Phase, run: &mut LiveRun) {
        let _ = meter::take();
        match phase {
            Phase::Drag(n) => {
                let mut st = self.state.borrow_mut();
                let Some(b) = st.doc.brushes().get(n) else {
                    return;
                };
                let (lo, hi) = b.bounds();
                let (a, c) = View::Top.axes();
                run.from = [(lo[a] + hi[a]) * 0.5, (lo[c] + hi[c]) * 0.5];
                let id = b.id;
                st.set_tool(Tool::Select);
                st.selection = Some(Selection::Brush(id));
                st.grid = 0.125;
                st.pointer_down(drag_at(run.from, 0));
            }
            Phase::Sculpt => {
                self.state.borrow_mut().set_tool(Tool::Voxel);
                self.pane3d().push_click(Click {
                    nx: 0.5,
                    ny: 0.5,
                    aspect: 1.6,
                    right: false,
                    ctrl: false,
                    alt: false,
                });
            }
            Phase::Fly => run.eye = self.state.borrow().eye.map_or([0.0; 3], |e| e.pos),
            Phase::Settle | Phase::Idle => {}
        }
    }

    fn live_gesture(&mut self, phase: Phase, run: &LiveRun) {
        let i = run.steps;
        match phase {
            Phase::Drag(_) => {
                self.state.borrow_mut().pointer_move(drag_at(run.from, i));
                if self.state.borrow().drag.is_some() {
                    self.refresh_gesture();
                }
            }
            Phase::Sculpt => self.pane3d().push_drag(PaneDrag {
                nx: 0.5 + 0.2 * sweep(i, 180) as f32,
                ny: 0.5,
                aspect: 1.6,
                released: false,
            }),
            Phase::Fly => {
                let e = run.eye;
                let ahead = 2.0 * (sweep(i, 240) + 1.0);
                let pos = mscent_math::Vec3::new(e[0] as f32, (e[1] + ahead) as f32, e[2] as f32);
                let yaw = 0.3 * sweep(i, 360) as f32;
                self.studio
                    .set_camera(crate::model::FlyCam::new(pos, yaw, 0.0));
            }
            Phase::Settle | Phase::Idle => {}
        }
    }

    fn live_end(&mut self, phase: Phase, run: &LiveRun) {
        match phase {
            Phase::Drag(_) => {
                self.state
                    .borrow_mut()
                    .pointer_up(drag_at(run.from, run.steps));
                self.refresh();
            }
            Phase::Sculpt => {
                self.pane3d().push_drag(PaneDrag {
                    nx: 0.5,
                    ny: 0.5,
                    aspect: 1.6,
                    released: true,
                });
                self.state.borrow_mut().set_tool(Tool::Select);
            }
            Phase::Settle | Phase::Idle | Phase::Fly => {}
        }
    }
}

/// Where the pointer is at step `i` of a drag from `from`, on a fine grid.
fn drag_at(from: [f64; 2], i: usize) -> Pointer {
    Pointer {
        view: View::Top,
        pos: [from[0] + 2.0 * (sweep(i, 120) + 1.0), from[1]],
        px: 0.02,
    }
}

#[cfg(test)]
mod tests {
    use super::sweep;

    #[test]
    fn sweep_goes_back_and_forth_within_one() {
        let v: Vec<f64> = (0..8).map(|i| sweep(i, 8)).collect();
        assert_eq!(v[0], -1.0);
        assert_eq!(v[4], 1.0);
        assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
    }
}
