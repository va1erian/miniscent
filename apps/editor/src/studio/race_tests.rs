//! Races between the UI thread and the render thread.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::Framebuffer;

use super::worker::Request;
use super::*;

/// Quits on its first two steps, each held until the test lets it go; later
/// steps return at once without quitting.
struct Gated {
    steps: u32,
    started: Sender<()>,
    gate: Receiver<()>,
}

impl Scene for Gated {
    fn restart(&mut self) {}

    fn step(&mut self, _dt: Duration, _events: &[Event], _stats: FrameStats) -> bool {
        self.steps += 1;
        if self.steps > 2 {
            return false;
        }
        let _ = self.started.send(());
        self.gate.recv_timeout(Duration::from_secs(10)).is_ok()
    }

    fn render(&mut self, _req: &Request) -> Option<&Framebuffer> {
        None
    }
}

/// The game asks to quit, the UI leaves Play, but a step that was already in
/// flight asks again before the render thread sees the mode change. That
/// stale request must not end the next Play at once.
#[test]
fn a_quit_request_left_over_from_a_finished_play_does_not_end_the_next_one() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let (started_tx, started) = channel();
    let (gate_tx, gate) = channel();
    let mut studio = Studio::new(&doc);
    studio.attach_scene(
        Box::new(Gated {
            steps: 0,
            started: started_tx,
            gate,
        }),
        &doc,
    );
    let wait = Duration::from_secs(10);
    studio.enter_play().expect("plays");
    started.recv_timeout(wait).expect("step 1 started");
    gate_tx.send(()).expect("release step 1");
    // Step 2 is now in flight; step 1's quit request is pending.
    started.recv_timeout(wait).expect("step 2 started");
    let end = Instant::now() + wait;
    loop {
        let t = studio.tick(&[], (64, 64), &doc);
        if t.left_play {
            break;
        }
        assert!(Instant::now() < end, "the quit request never arrived");
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(studio.mode, Mode::Edit);
    // Step 2 finishes and asks to quit after the UI already left Play.
    gate_tx.send(()).expect("release step 2");
    std::thread::sleep(Duration::from_millis(150));
    studio.enter_play().expect("plays again");
    let end = Instant::now() + Duration::from_millis(400);
    while Instant::now() < end {
        let t = studio.tick(&[], (64, 64), &doc);
        assert!(!t.left_play, "a stale quit request ended the new Play");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(studio.mode, Mode::Play);
}
