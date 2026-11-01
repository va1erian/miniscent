#![forbid(unsafe_code)]

//! The `winit` [`ApplicationHandler`] that drives [`WinitBackend`]: it creates
//! the real windows, translates platform input into portable events, and
//! presents a frame on redraw — a `softbuffer` copy of the software composite,
//! into which any GL content has already been rendered.
//!
//! The handler is a thin dispatcher: window creation and DPI live in
//! [`window`], input translation in [`input`] (with the pure mappings in
//! [`keymap`]), and frame presentation in [`present`].

mod double_click;
mod input;
mod keymap;
mod present;
mod window;

use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

use xui_core::backend::{Event, TimerId, WidgetId, WindowId};
use xui_core::message::Modifiers;

use super::software::RealWindow;
use super::{Shared, UserEvent};
use double_click::ClickTracker;
use window::dpi_from_scale;

/// Drives one `winit` event loop for a [`super::WinitBackend`].
struct App<'a> {
    shared: Rc<Shared>,
    windows: HashMap<u64, RealWindow>,
    /// The last pointer position in window pixels, for button messages `winit`
    /// sends without one.
    cursor: (f64, f64),
    modifiers: Modifiers,
    /// Recognizes the second press of a double-click, which `winit` never
    /// reports itself.
    double_click: ClickTracker,
    /// The primary window's deferred app builder and the raw id of the window
    /// that must exist before it runs; taken out the first time that window is
    /// created. A secondary window opened later is a separate path and is not
    /// deferred.
    on_ready: Option<(u64, &'a mut dyn FnMut())>,
}

impl App<'_> {
    fn window_id(raw: u64) -> WindowId {
        WindowId::from_raw(raw)
    }

    /// Fires any due timers and arms the next wake-up.
    fn timers(&self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        let due: Vec<(usize, WindowId)> = self
            .shared
            .timers
            .borrow()
            .iter()
            .filter(|(_, (_, at, _))| *at <= now)
            .map(|(id, (window, _, _))| (*id, *window))
            .collect();
        for (id, window) in due {
            // A callback run earlier in this batch may have killed the timer
            // or closed its window. Reschedule before delivering so this
            // timer's own callback may kill it.
            let rescheduled = match self.shared.timers.borrow_mut().get_mut(&id) {
                Some(timer) => {
                    timer.1 = now + timer.2;
                    true
                }
                None => false,
            };
            if !rescheduled {
                continue;
            }
            self.shared
                .deliver(window, WidgetId::NONE, &Event::Timer { id: TimerId(id) });
        }
        let next = self
            .shared
            .timers
            .borrow()
            .values()
            .map(|(_, at, _)| *at)
            .min();
        match next {
            Some(at) => event_loop.set_control_flow(ControlFlow::WaitUntil(at)),
            None => event_loop.set_control_flow(ControlFlow::Wait),
        }
    }
}

impl ApplicationHandler<UserEvent> for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.create_windows(event_loop);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let raw = window_id_index(self, window_id);
        let Some(raw) = raw else {
            return;
        };
        match event {
            WindowEvent::RedrawRequested => self.redraw(raw),
            WindowEvent::CloseRequested => {
                let primary = self
                    .shared
                    .windows
                    .borrow()
                    .get(&raw)
                    .is_some_and(|state| state.primary);
                let handled =
                    self.shared
                        .deliver(Self::window_id(raw), WidgetId::NONE, &Event::Close);
                if !handled && primary {
                    event_loop.exit();
                }
            }
            // A window that was hidden or shown again has to be handed the
            // whole frame: the OS may have discarded what it showed.
            WindowEvent::Occluded(_) => {
                if let Some(real) = self.windows.get_mut(&raw) {
                    real.buffer_stale = true;
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(state) = self.shared.windows.borrow_mut().get_mut(&raw) {
                    state.size = (size.width.max(1), size.height.max(1));
                }
                self.shared
                    .nodes
                    .borrow_mut()
                    .invalidate_window(Self::window_id(raw));
                self.shared.deliver(
                    Self::window_id(raw),
                    WidgetId::NONE,
                    &Event::Resize {
                        width: size.width as i32,
                        height: size.height as i32,
                    },
                );
                self.redraw(raw);
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                let dpi = dpi_from_scale(scale_factor);
                self.set_dpi(raw, dpi);
                // `winit` may report the scale change before the matching
                // `Resized`, so adopt the fresh backing size now; the next
                // frame then presents at the new DPI.
                self.adopt_inner_size(raw);
                self.shared
                    .nodes
                    .borrow_mut()
                    .invalidate_window(Self::window_id(raw));
                // `winit` gives only the scale factor, not the OS's suggested
                // window rectangle, so the suggestion is empty and the app
                // re-lays-out from `Ui::dpi` instead.
                self.shared.deliver(
                    Self::window_id(raw),
                    WidgetId::NONE,
                    &Event::DpiChanged {
                        dpi,
                        suggested: xui_core::Rect::default(),
                    },
                );
                self.redraw(raw);
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_moved(raw, position.x, position.y);
            }
            WindowEvent::CursorLeft { .. } => self.apply_cursor(raw, None),
            WindowEvent::MouseInput { state, button, .. } => {
                self.mouse_input(raw, state, button);
            }
            WindowEvent::MouseWheel { delta, .. } => self.mouse_wheel(raw, delta),
            WindowEvent::ModifiersChanged(modifiers) => {
                let state = modifiers.state();
                self.modifiers = Modifiers {
                    ctrl: state.control_key(),
                    shift: state.shift_key(),
                    alt: state.alt_key(),
                    win: state.super_key(),
                };
            }
            WindowEvent::KeyboardInput { event, .. } => {
                self.keyboard_input(
                    raw,
                    event.state,
                    event.repeat,
                    &event.logical_key,
                    event.text.as_deref(),
                );
            }
            _ => {}
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        // A window opened while the loop runs (a secondary window) has no OS
        // window yet; create it before its wake event is delivered.
        self.create_windows(event_loop);
        if self.shared.quit.get() {
            event_loop.exit();
            return;
        }
        match event {
            UserEvent::Wake(window) => {
                self.shared.deliver(window, WidgetId::NONE, &Event::Wake);
            }
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.shared.quit.get() {
            event_loop.exit();
            return;
        }
        // Catch a window opened without a wake (for example a static one).
        self.create_windows(event_loop);
        // A window the backend closed this turn has no state left; drop its OS
        // window so it leaves the screen instead of staying frozen.
        self.reconcile_windows();
        self.timers(event_loop);
    }
}

/// The raw id of the window `winit` handed the event for, or `None` for a
/// window the backend does not track.
fn window_id_index(app: &App<'_>, window_id: winit::window::WindowId) -> Option<u64> {
    app.shared
        .windows
        .borrow()
        .iter()
        .find(|(_, state)| {
            state
                .window
                .as_ref()
                .is_some_and(|window| window.id() == window_id)
        })
        .map(|(raw, _)| *raw)
}

/// Runs the event loop until the windows close or the app quits.
pub(crate) fn run(event_loop: EventLoop<UserEvent>, shared: Rc<Shared>) {
    let mut app = App {
        shared,
        windows: HashMap::new(),
        cursor: (0.0, 0.0),
        modifiers: Modifiers::NONE,
        double_click: ClickTracker::system(),
        on_ready: None,
    };
    let _ = event_loop.run_app(&mut app);
}

/// Runs the event loop, invoking `on_ready` once `window`'s real `winit` window
/// exists and its DPI is known, then pumping until quit.
pub(crate) fn run_with(
    event_loop: EventLoop<UserEvent>,
    shared: Rc<Shared>,
    window: WindowId,
    on_ready: &mut dyn FnMut(),
) {
    let mut app = App {
        shared,
        windows: HashMap::new(),
        cursor: (0.0, 0.0),
        modifiers: Modifiers::NONE,
        double_click: ClickTracker::system(),
        on_ready: Some((window.raw(), on_ready)),
    };
    let _ = event_loop.run_app(&mut app);
}
