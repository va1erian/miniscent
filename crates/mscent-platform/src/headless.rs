//! The headless backend: scripted input, a virtual clock and frame capture.
//! Always compiled; this is what tests and `--screenshot` runs use.

use std::time::Duration;

use crate::Platform;
use crate::key::Event;
use crate::script::{Action, Script};

/// One presented frame, `0xAARRGGBB` pixels.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u32>,
}

/// A platform with no window. Time is virtual: it advances by `frame_dt`
/// on every [`Platform::poll_events`], so a run is fully deterministic.
#[derive(Debug)]
pub struct HeadlessPlatform {
    size: (u32, u32),
    frame_dt: Duration,
    script: Script,
    next_action: usize,
    time: Duration,
    polled: u64,
    frame_limit: Option<u64>,
    quit: bool,
    captured: bool,
    presented: u64,
    last: Option<Frame>,
    keep_all: bool,
    all: Vec<Frame>,
}

impl HeadlessPlatform {
    /// A `width` x `height` virtual window stepping `frame_dt` per frame.
    pub fn new(width: u32, height: u32, frame_dt: Duration, script: Script) -> Self {
        Self {
            size: (width, height),
            frame_dt,
            script,
            next_action: 0,
            time: Duration::ZERO,
            polled: 0,
            frame_limit: None,
            quit: false,
            captured: false,
            presented: 0,
            last: None,
            keep_all: false,
            all: Vec::new(),
        }
    }

    /// Ends the run (`should_quit`) once this many frames were presented.
    pub fn with_frame_limit(mut self, frames: u64) -> Self {
        self.frame_limit = Some(frames);
        self
    }

    /// Keeps every presented frame, not only the last. Memory grows with the run.
    pub fn keep_all_frames(mut self) -> Self {
        self.keep_all = true;
        self
    }

    pub fn last_frame(&self) -> Option<&Frame> {
        self.last.as_ref()
    }

    /// All kept frames (empty unless [`Self::keep_all_frames`] was used).
    pub fn frames(&self) -> &[Frame] {
        &self.all
    }

    pub fn frames_presented(&self) -> u64 {
        self.presented
    }

    /// Whether the game last asked for the mouse to be captured.
    pub fn mouse_captured(&self) -> bool {
        self.captured
    }
}

impl Platform for HeadlessPlatform {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn present(&mut self, pixels: &[u32], width: u32, height: u32) {
        let expected = (width as usize).checked_mul(height as usize);
        if expected != Some(pixels.len()) {
            return;
        }
        self.presented += 1;
        let frame = Frame {
            width,
            height,
            pixels: pixels.to_vec(),
        };
        if self.keep_all {
            self.all.push(frame.clone());
        }
        self.last = Some(frame);
        if self.frame_limit.is_some_and(|n| self.presented >= n) {
            self.quit = true;
        }
    }

    fn poll_events(&mut self) -> Vec<Event> {
        if self.polled > 0 {
            self.time += self.frame_dt;
        }
        self.polled += 1;
        let now = self.time.as_secs_f64();
        let mut events = Vec::new();
        while let Some(&(t, action)) = self.script.actions().get(self.next_action) {
            if t > now {
                break;
            }
            self.next_action += 1;
            match action {
                Action::Input(e) => events.push(e),
                Action::Quit => self.quit = true,
            }
        }
        events
    }

    fn now(&self) -> Duration {
        self.time
    }

    fn set_mouse_captured(&mut self, captured: bool) {
        self.captured = captured;
    }

    fn should_quit(&self) -> bool {
        self.quit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::Key;

    fn run(script: &str, frames: usize) -> Vec<Vec<Event>> {
        let s = Script::parse(script).unwrap();
        let mut p = HeadlessPlatform::new(8, 8, Duration::from_millis(100), s);
        (0..frames).map(|_| p.poll_events()).collect()
    }

    #[test]
    fn events_arrive_on_the_frame_their_time_falls_in() {
        let f = run("at 0.25s hold W\nat 0.5s release W\n", 7);
        let down = Event::Key {
            code: Key::Char('W'),
            pressed: true,
        };
        assert!(f[0].is_empty() && f[1].is_empty() && f[2].is_empty());
        assert_eq!(f[3], vec![down]);
        assert_eq!(f[5].len(), 1);
        assert!(f[6].is_empty());
    }

    #[test]
    fn clock_is_virtual_and_quit_comes_from_the_script() {
        let s = Script::parse("at 0.2s quit").unwrap();
        let mut p = HeadlessPlatform::new(4, 4, Duration::from_millis(100), s);
        p.poll_events();
        assert_eq!(p.now(), Duration::ZERO);
        p.poll_events();
        assert!(!p.should_quit());
        p.poll_events();
        assert_eq!(p.now(), Duration::from_millis(200));
        assert!(p.should_quit());
    }

    #[test]
    fn present_captures_pixels_and_the_frame_limit_quits() {
        let mut p = HeadlessPlatform::new(2, 1, Duration::from_millis(10), Script::empty())
            .with_frame_limit(2)
            .keep_all_frames();
        p.present(&[1, 2], 2, 1);
        p.present(&[1], 2, 1); // wrong size: ignored
        assert!(!p.should_quit());
        p.present(&[3, 4], 2, 1);
        assert!(p.should_quit());
        assert_eq!(p.frames().len(), 2);
        assert_eq!(p.last_frame().unwrap().pixels, vec![3, 4]);
    }
}
