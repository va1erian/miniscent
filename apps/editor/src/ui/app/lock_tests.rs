//! Pointer lock through xui's offscreen backend, which records lock requests:
//! the real editor is built, input is injected as the window system delivers
//! it, and the camera and the lock are read back.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mscent_platform::Event as PEvent;
use xui::app::run_app;
use xui::backend::{Backend, Event, PlatformSpec, WindowId};
use xui::xui_canvas::OffscreenBackend;
use xui::{Dip, Key, Modifiers, MouseButton};

use super::*;

/// Hosts the editor so the test keeps a handle on it.
struct Host(Rc<RefCell<Editor>>);

impl App for Host {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        self.0.borrow_mut().update(msg, ui);
    }
}

struct Session<'a> {
    backend: &'a OffscreenBackend,
    window: WindowId,
    ui: Ui<Msg>,
    editor: Rc<RefCell<Editor>>,
}

impl Session<'_> {
    fn inject(&self, event: Event) {
        self.backend.inject(self.window, event);
        self.backend.pump(self.window);
    }

    fn press(&self, key: Key) {
        let modifiers = Modifiers::NONE;
        self.inject(Event::KeyDown {
            key,
            modifiers,
            repeat: 1,
            system: false,
        });
        self.inject(Event::KeyUp {
            key,
            modifiers,
            system: false,
        });
    }

    /// Steps the app until `done` holds.
    fn wait(&self, what: &str, done: impl Fn(&Editor) -> bool) {
        for _ in 0..30_000 {
            self.ui.emit(Msg::Step);
            self.backend.pump(self.window);
            if done(&self.editor.borrow()) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for {what}");
    }

    /// A window point over the node `id`: bounds are relative to the
    /// enclosing splitter, so look for the point that locks the pointer there.
    fn inside(&self, id: xui::backend::WidgetId) -> (i32, i32) {
        for y in (10..800).step_by(25) {
            for x in (10..1280).step_by(25) {
                self.inject(Event::MouseDown {
                    x,
                    y,
                    button: MouseButton::Right,
                    modifiers: NONE,
                });
                let hit = self.backend.pointer_lock() == Some(id);
                self.inject(Event::MouseUp {
                    x,
                    y,
                    button: MouseButton::Right,
                    modifiers: NONE,
                });
                if hit {
                    return (x, y);
                }
            }
        }
        panic!("no point over the node");
    }

    fn wait_message(&self, prefix: &str) {
        self.wait(prefix, |e| e.state.borrow().message.starts_with(prefix));
    }

    /// Waits for the picture that needs no command to appear.
    fn wait_preview(&self) {
        self.wait("the preview", |e| {
            e.studio.preview(&e.state.borrow().doc) == Preview::Full
        });
    }
}

/// The UI handle and editor, handed from the build closure to the script.
type Slot = Rc<RefCell<Option<(Ui<Msg>, Rc<RefCell<Editor>>)>>>;

fn run(script: impl FnOnce(&Session<'_>) + 'static) {
    let backend = Rc::new(OffscreenBackend::new());
    let slot: Slot = Rc::default();
    let ran = Rc::new(Cell::new(false));
    let (weak, hook_slot, hook_ran) = (Rc::downgrade(&backend), slot.clone(), ran.clone());
    backend.set_run_hook(move || {
        let (Some(backend), Some((ui, editor))) = (weak.upgrade(), hook_slot.borrow_mut().take())
        else {
            return;
        };
        let session = Session {
            backend: &backend,
            window: ui.window(),
            ui,
            editor,
        };
        let _ = backend.render(session.window);
        script(&session);
        hook_ran.set(true);
    });
    let handle: Rc<dyn Backend> = backend.clone();
    let spec = PlatformSpec::new("lock").size(Dip(1280.0), Dip(800.0));
    run_app(handle, spec, move |ui| {
        let doc = Document::from_level(&mscent_world::showcase_level());
        let editor = Rc::new(RefCell::new(Editor::build(ui, doc).expect("editor builds")));
        *slot.borrow_mut() = Some((ui.clone(), editor.clone()));
        Host(editor)
    })
    .expect("run");
    assert!(ran.get(), "the script ran to the end");
}

const NONE: Modifiers = Modifiers::NONE;

fn motion(dx: f32, dy: f32) -> PEvent {
    PEvent::MouseMotion { dx, dy }
}

#[test]
fn fly_look_locks_the_pointer_and_turns_the_camera_from_relative_motion() {
    run(|s| {
        let probe = s.editor.borrow().probe();
        let pane = probe.view_pane();
        let id = xui::widget::Placeable::id(&*pane);
        let (x, y) = s.inside(id);
        let down = |button| Event::MouseDown {
            x,
            y,
            button,
            modifiers: NONE,
        };
        let up = |button| Event::MouseUp {
            x,
            y,
            button,
            modifiers: NONE,
        };

        s.wait_preview();

        // The right button locks the pointer; the left one does not.
        s.inject(down(MouseButton::Left));
        assert_eq!(s.backend.pointer_lock(), None);
        s.inject(up(MouseButton::Left));
        s.inject(down(MouseButton::Right));
        assert_eq!(s.backend.pointer_lock(), Some(id));
        pane.take_events();

        // Until a delta arrives, position differences are the fallback look;
        // after one, they are ignored so motion is not counted twice.
        let at = |dx| Event::MouseMove {
            x: x + dx,
            y,
            modifiers: NONE,
        };
        s.inject(at(10));
        assert_eq!(pane.take_events(), [motion(10.0, 0.0)]);
        s.inject(Event::PointerDelta { dx: 5, dy: 3 });
        assert_eq!(pane.take_events(), [motion(5.0, 3.0)]);
        s.inject(at(20));
        assert_eq!(pane.take_events(), []);

        // Deltas turn the camera: right and up in, yaw decreases, pitch rises.
        let before = s.editor.borrow().studio.fly_camera();
        s.inject(Event::PointerDelta { dx: 40, dy: -25 });
        s.wait("the camera to turn", |e| {
            e.studio.fly_camera().yaw != before.yaw
        });
        let after = s.editor.borrow().studio.fly_camera();
        assert!(after.yaw < before.yaw, "{} -> {}", before.yaw, after.yaw);
        assert!(after.pitch > before.pitch);

        // Releasing the button gives the pointer back.
        s.inject(up(MouseButton::Right));
        assert_eq!(s.backend.pointer_lock(), None);

        // A lock the backend ends (Esc, focus loss) stops the look at once.
        s.inject(down(MouseButton::Right));
        assert_eq!(s.backend.pointer_lock(), Some(id));
        s.inject(Event::PointerLockLost);
        pane.take_events();
        s.inject(at(30));
        assert_eq!(pane.take_events(), [], "no look after the lock was lost");
        s.inject(up(MouseButton::Right));
        assert_eq!(s.backend.pointer_lock(), None);
    });
}

#[test]
fn play_locks_the_pointer_and_releases_it_on_esc_and_when_the_backend_ends_it() {
    run(|s| {
        let probe = s.editor.borrow().probe();
        let play = probe.play_pane();
        let id = xui::widget::Placeable::id(&*play);

        s.wait_preview();
        assert_eq!(s.backend.pointer_lock(), None);

        // Entering Play locks the pointer and mouse-look reads the deltas.
        s.press(Key::F5);
        s.wait_message("Playing");
        assert_eq!(s.backend.pointer_lock(), Some(id));
        let host = xui::widget::Placeable::id(&*s.editor.borrow().work_host.get());
        assert!(
            !s.ui.is_visible(host),
            "the editor is not painted under Play"
        );
        // Play's input goes on to the render thread as it arrives, not at the
        // next tick: here to a queue of the test's own.
        let inbox = crate::studio::worker::Inbox::default();
        play.set_forward(Some(crate::studio::worker::Input::new(inbox.clone())));
        s.inject(Event::PointerDelta { dx: 7, dy: -3 });
        assert_eq!(*inbox.lock().unwrap(), [motion(7.0, -3.0)]);
        assert_eq!(play.take_events(), [], "nothing waits for the tick");

        // Esc leaves Play and releases the pointer.
        s.press(Key::ESCAPE);
        s.wait_message("Back in the editor");
        assert_eq!(s.backend.pointer_lock(), None);
        assert!(s.ui.is_visible(host), "the editor is back");

        // The backend ends the lock itself (Esc swallowed by the grab, or
        // focus lost): Play ends and the pointer stays released.
        s.press(Key::F5);
        s.wait_message("Playing");
        assert_eq!(s.backend.pointer_lock(), Some(id));
        s.inject(Event::PointerLockLost);
        s.wait_message("Back in the editor");
        assert_eq!(s.backend.pointer_lock(), None);
        assert_eq!(s.editor.borrow().studio.mode, Mode::Edit);
    });
}

#[test]
fn a_play_queued_before_the_workspace_opens_never_starts_under_it() {
    run(|s| {
        // The level is not built yet, so F5 waits for it.
        s.ui.emit(Msg::Menu(cmd::PLAY));
        s.ui.emit(Msg::Menu(cmd::TEX_OPEN));
        s.backend.pump(s.window);
        assert!(s.editor.borrow().tex.is_open());
        s.wait_preview();
        // The picture is up: the Play that was waiting for it would start now.
        for _ in 0..20 {
            s.ui.emit(Msg::Step);
            s.backend.pump(s.window);
        }
        let editor = s.editor.borrow();
        assert_eq!(editor.studio.mode, Mode::Edit);
        assert!(editor.tex.is_open(), "the workspace is still up");
    });
}

#[test]
fn play_from_any_route_closes_the_workspace_and_its_shortcuts_stay_the_levels() {
    run(|s| {
        s.wait_preview();
        s.ui.emit(Msg::Menu(cmd::TEX_OPEN));
        s.backend.pump(s.window);
        assert!(s.editor.borrow().tex.is_open());
        // Not through F5, which the workspace ignores: Play begun some other way.
        s.editor.borrow_mut().begin_play();
        {
            let editor = s.editor.borrow();
            assert_eq!(editor.studio.mode, Mode::Play);
            assert!(!editor.tex.is_open(), "the workspace went first");
        }
        // Ctrl+S in the game is not a Save of a material.
        s.ui.emit(Msg::Menu(cmd::TEX_SAVE));
        s.ui.emit(Msg::Menu(cmd::SAVE));
        s.backend.pump(s.window);
        let editor = s.editor.borrow();
        assert!(!editor.tex.studio.borrow().is_baking());
        assert_eq!(editor.studio.mode, Mode::Play, "and still playing");
    });
}
