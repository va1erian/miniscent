//! The UI tick never waits for the worker. No clock is involved in the proof: the
//! locks are held by another thread until the UI thread is done, so a call that
//! waited for them could only get through once the holder gave up, and that is the
//! failure. The evaluation is held up the same way, behind its gate. What ticks
//! cost in a drag is measured separately and as a rule, in the way of
//! `studio/tick_costs.rs`.

use std::sync::mpsc::{RecvTimeoutError, channel};
use std::time::Duration;

use mscent_texgen::ops::{self, Value};

use super::tests::{set_page, settle, studio};
use super::*;
use crate::studio::tick_costs::TickCosts;

/// How long a holder lets go of a lock for: far beyond any tick, however busy the
/// machine, so only a thread that really waits gets to see it end.
const PATIENCE: Duration = Duration::from_secs(30);
/// Ticks to run while something is held up.
const TICKS: u32 = 200;

const PAGE: &str =
    "op 0 0 1 noise\nop 0 1 1 out-albedo\nop 1 0 1 flat color=0.5,0.5,0.5\nop 1 1 1 out-height\n";

fn colour_index(s: &TexStudio) -> usize {
    let spec = s.doc().expect("open").recipe().blocks[2].spec;
    spec.params
        .iter()
        .position(|p| matches!(p.kind, ops::ParamKind::Color { .. }))
        .expect("a colour")
}

fn edit(s: &mut TexStudio, i: u32) {
    let index = colour_index(s);
    let v = (i % 100) as f32 / 100.0;
    let value = Value::Color([v, 0.5, 0.5, 1.0]);
    s.edit(|d| d.set_param(2, index, value, true))
        .expect("edits");
    s.note_drag(true);
}

/// Runs `f` while another thread holds what `hold` locks, until `f` is done.
/// Fails when `f` got there by waiting for it.
#[track_caller]
fn while_held<L: Send + 'static>(locks: L, hold: fn(&L, &mut dyn FnMut()), f: impl FnOnce()) {
    let (locked, wait) = channel();
    let (done, until) = channel::<()>();
    let t = std::thread::spawn(move || {
        let mut gave_up = false;
        hold(&locks, &mut || {
            locked.send(()).expect("signal");
            gave_up = matches!(until.recv_timeout(PATIENCE), Err(RecvTimeoutError::Timeout));
        });
        gave_up
    });
    wait.recv().expect("locked");
    f();
    // A holder that gave up has hung up already; its answer says so below.
    let _ = done.send(());
    let gave_up = t.join().expect("holder");
    assert!(!gave_up, "the UI thread waited for the worker");
}

#[test]
fn ticks_and_edits_do_not_wait_for_the_mailboxes() {
    let mut s = studio();
    set_page(&mut s, 64, PAGE);
    settle(&mut s);
    let locks = s.worker.as_ref().expect("worker").locks();
    while_held(
        locks,
        |l, inner| {
            let _held = l.mailboxes();
            inner();
        },
        || {
            for i in 0..TICKS {
                edit(&mut s, i);
                let tick = s.tick();
                assert!(tick.picture.is_none(), "nothing can have come out");
            }
        },
    );
    // The mailboxes are free again: the edits were not lost.
    s.note_drag(false);
    settle(&mut s);
}

#[test]
fn ticks_go_on_while_an_evaluation_cannot_finish() {
    let mut s = studio();
    set_page(&mut s, 64, PAGE);
    settle(&mut s);
    let locks = s.worker.as_ref().expect("worker").locks();
    while_held(
        locks,
        |l, inner| {
            let _held = l.gate();
            inner();
        },
        || {
            // A request is taken, and sticks at the gate; more pile up on it.
            for i in 0..TICKS {
                edit(&mut s, i);
                assert!(s.tick().picture.is_none(), "it cannot have finished");
            }
            assert!(!s.settled());
        },
    );
    s.note_drag(false);
    settle(&mut s);
}

#[test]
fn a_bake_held_up_does_not_stop_ticks_and_arrives_afterwards() {
    let mut s = studio();
    set_page(&mut s, 64, PAGE);
    settle(&mut s);
    let locks = s.worker.as_ref().expect("worker").locks();
    while_held(
        locks,
        |l, inner| {
            let _held = l.mailboxes();
            inner();
        },
        || {
            assert!(s.apply());
            for _ in 0..TICKS {
                s.tick();
                assert!(s.is_baking(), "the mailbox was never free");
            }
        },
    );
    let end = std::time::Instant::now() + PATIENCE;
    while s.is_baking() {
        s.tick();
        assert!(std::time::Instant::now() < end, "never arrived");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(s.table().expect("table").find("test").is_some());
}

#[test]
fn ticks_are_cheap_during_a_drag() {
    let mut s = studio();
    set_page(&mut s, 1024, PAGE);
    settle(&mut s);
    let mut costs = TickCosts::default();
    for i in 0..600 {
        costs.timed(|| {
            edit(&mut s, i);
            s.tick()
        });
        std::thread::sleep(Duration::from_millis(1));
    }
    costs.assert_cheap();
    s.note_drag(false);
    settle(&mut s);
}

/// Clock-free: the worker sits at the gate with the first request while the next
/// ones arrive, and a run told to stop gives no answer at all, so the only answers
/// that can ever appear are of the newest request.
#[test]
fn a_stale_run_is_cancelled_and_its_answer_never_shows() {
    let mut s = studio();
    let page = "op 0 0 1 flat color=1,0,0\nop 0 1 1 out-albedo\n";
    set_page(&mut s, 32, page);
    settle(&mut s);
    s.set_tab(Tab::Albedo);
    let colour = 0;
    let locks = s.worker.as_ref().expect("worker").locks();
    {
        let _gate = locks.gate();
        // One is taken and sticks at the gate, the rest replace each other.
        for v in [0.1, 0.2, 0.3, 0.4] {
            let value = Value::Color([v, 0.0, 0.0, 1.0]);
            s.edit(|d| d.set_param(0, colour, value, false))
                .expect("edits");
            s.tick();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    let newest = s.asked_seq;
    let end = std::time::Instant::now() + PATIENCE;
    let mut last = None;
    while !s.settled() {
        if let Some(p) = s.tick().picture {
            assert!(
                p.seq == newest,
                "seq {} of a stale run; newest {newest}",
                p.seq
            );
            last = Some(p);
        }
        assert!(std::time::Instant::now() < end, "never settled");
        std::thread::sleep(Duration::from_millis(1));
    }
    let picture = last.expect("a picture");
    assert_eq!(picture.rgba[0], 102, "0.4 of 255");
}

#[test]
fn closing_a_workspace_cancels_what_is_running_and_the_thread_stops_with_it() {
    let mut s = studio();
    set_page(&mut s, 1024, PAGE);
    s.tick();
    s.set_open(false);
    assert!(s.worker.is_some());
    drop(s);
}
