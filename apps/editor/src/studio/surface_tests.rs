//! The Play window's way through the render thread, with stand-ins for the
//! window and the scene, so no GPU is needed.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use miniscent_game::FrameStats;
use mscent_platform::Event;
use mscent_render::Framebuffer;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, WindowHandle,
};

use super::worker::{Request, Surface};
use super::*;

/// A window with no handles, for a scene that never draws into it.
struct Nowhere;

impl HasWindowHandle for Nowhere {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

impl HasDisplayHandle for Nowhere {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Err(HandleError::Unavailable)
    }
}

/// What a scene was told about its window, newest last.
type Told = Arc<Mutex<Vec<bool>>>;

/// A scene that records each surface it is given and gives the window up when
/// the test says so.
#[derive(Default)]
struct Recorder {
    told: Told,
    lose: Arc<Mutex<bool>>,
    fb: Option<Framebuffer>,
}

impl Scene for Recorder {
    fn restart(&mut self) {}

    fn step(&mut self, _: Duration, _: &[Event], _: FrameStats) -> bool {
        false
    }

    fn render(&mut self, req: &Request) -> Option<&Framebuffer> {
        self.fb = Framebuffer::new(req.w, req.h);
        self.fb.as_ref()
    }

    fn set_surface(&mut self, window: Option<Surface>) {
        self.told.lock().unwrap().push(window.is_some());
    }

    fn surface_lost(&mut self) -> bool {
        std::mem::take(&mut *self.lose.lock().unwrap())
    }
}

const PANE: (u32, u32) = (64, 48);

/// Ticks until `done`, which must happen well within a few seconds.
fn until(
    studio: &mut Studio,
    doc: &Document,
    what: &str,
    mut done: impl FnMut(&mut Studio) -> bool,
) {
    for _ in 0..2000 {
        studio.tick(&[], PANE, doc);
        if done(studio) {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("timed out waiting for {what}");
}

fn last(told: &Told) -> Option<bool> {
    told.lock().unwrap().last().copied()
}

fn window() -> Surface {
    Arc::new(Nowhere)
}

#[test]
fn the_window_reaches_the_scene_and_a_new_scene_gets_it_too() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let mut studio = Studio::new(&doc);
    let first = Recorder::default();
    let told = first.told.clone();
    studio.attach_scene(Box::new(first), &doc);
    studio.set_surface(Some(window()));
    until(&mut studio, &doc, "the window", |_| {
        last(&told) == Some(true)
    });

    let second = Recorder::default();
    let told_again = second.told.clone();
    studio.attach_scene(Box::new(second), &doc);
    until(&mut studio, &doc, "the window again", |_| {
        last(&told_again) == Some(true)
    });

    studio.set_surface(None);
    until(&mut studio, &doc, "no window", |_| {
        last(&told_again) == Some(false)
    });
}

#[test]
fn a_window_the_scene_gives_up_is_reported_once_and_forgotten() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let mut studio = Studio::new(&doc);
    let scene = Recorder::default();
    let lose = scene.lose.clone();
    studio.attach_scene(Box::new(scene), &doc);
    studio.set_surface(Some(window()));
    assert!(studio.has_surface());
    *lose.lock().unwrap() = true;
    until(&mut studio, &doc, "the loss", Studio::take_surface_lost);
    assert!(!studio.has_surface(), "the window is forgotten");
    for _ in 0..20 {
        studio.tick(&[], PANE, &doc);
        assert!(!studio.take_surface_lost(), "reported once");
    }
}

#[test]
fn a_loss_left_over_from_one_window_is_not_the_next_ones() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let mut studio = Studio::new(&doc);
    let scene = Recorder::default();
    let lose = scene.lose.clone();
    studio.attach_scene(Box::new(scene), &doc);
    *lose.lock().unwrap() = true;
    // The loss lands while no one asks, as between Esc and the next F5.
    until(&mut studio, &doc, "a frame", |_| !*lose.lock().unwrap());
    studio.set_surface(Some(window()));
    assert!(!studio.take_surface_lost());
    assert!(studio.has_surface());
}
