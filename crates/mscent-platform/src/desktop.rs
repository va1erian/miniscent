//! The desktop backend: a winit window presented through softbuffer.
//! Behind the `desktop` cargo feature.

use std::sync::Arc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton as WinitButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::platform::pump_events::{EventLoopExtPumpEvents, PumpStatus};
use winit::window::{CursorGrabMode, Window, WindowId};

use crate::Platform;
use crate::key::{Event, Key, MouseButton};
use crate::scale::blit_fit;

/// Window settings.
#[derive(Clone, Debug)]
pub struct DesktopConfig {
    pub title: String,
    /// Initial inner size in physical pixels.
    pub width: u32,
    pub height: u32,
}

/// The window, its softbuffer surface and the events collected since the
/// last poll.
struct App {
    config: DesktopConfig,
    window: Option<Arc<Window>>,
    surface: Option<Surface<Arc<Window>, Arc<Window>>>,
    events: Vec<Event>,
    size: (u32, u32),
    focused: bool,
    quit: bool,
    failure: Option<String>,
}

fn map_key(code: KeyCode) -> Option<Key> {
    use KeyCode as K;
    let letter = |c: char| Some(Key::Char(c));
    Some(match code {
        K::KeyA => return letter('A'),
        K::KeyB => return letter('B'),
        K::KeyC => return letter('C'),
        K::KeyD => return letter('D'),
        K::KeyE => return letter('E'),
        K::KeyF => return letter('F'),
        K::KeyG => return letter('G'),
        K::KeyH => return letter('H'),
        K::KeyI => return letter('I'),
        K::KeyJ => return letter('J'),
        K::KeyK => return letter('K'),
        K::KeyL => return letter('L'),
        K::KeyM => return letter('M'),
        K::KeyN => return letter('N'),
        K::KeyO => return letter('O'),
        K::KeyP => return letter('P'),
        K::KeyQ => return letter('Q'),
        K::KeyR => return letter('R'),
        K::KeyS => return letter('S'),
        K::KeyT => return letter('T'),
        K::KeyU => return letter('U'),
        K::KeyV => return letter('V'),
        K::KeyW => return letter('W'),
        K::KeyX => return letter('X'),
        K::KeyY => return letter('Y'),
        K::KeyZ => return letter('Z'),
        K::Digit0 => return letter('0'),
        K::Digit1 => return letter('1'),
        K::Digit2 => return letter('2'),
        K::Digit3 => return letter('3'),
        K::Digit4 => return letter('4'),
        K::Digit5 => return letter('5'),
        K::Digit6 => return letter('6'),
        K::Digit7 => return letter('7'),
        K::Digit8 => return letter('8'),
        K::Digit9 => return letter('9'),
        K::Escape => Key::Escape,
        K::Enter | K::NumpadEnter => Key::Enter,
        K::Space => Key::Space,
        K::Tab => Key::Tab,
        K::Backspace => Key::Backspace,
        K::Delete => Key::Delete,
        K::ShiftLeft | K::ShiftRight => Key::Shift,
        K::ControlLeft | K::ControlRight => Key::Ctrl,
        K::AltLeft | K::AltRight => Key::Alt,
        K::ArrowUp => Key::Up,
        K::ArrowDown => Key::Down,
        K::ArrowLeft => Key::Left,
        K::ArrowRight => Key::Right,
        K::F1 => Key::F(1),
        K::F2 => Key::F(2),
        K::F3 => Key::F(3),
        K::F4 => Key::F(4),
        K::F5 => Key::F(5),
        K::F6 => Key::F(6),
        K::F7 => Key::F(7),
        K::F8 => Key::F(8),
        K::F9 => Key::F(9),
        K::F10 => Key::F(10),
        K::F11 => Key::F(11),
        K::F12 => Key::F(12),
        _ => return None,
    })
}

fn map_button(b: WinitButton) -> Option<MouseButton> {
    match b {
        WinitButton::Left => Some(MouseButton::Left),
        WinitButton::Right => Some(MouseButton::Right),
        WinitButton::Middle => Some(MouseButton::Middle),
        _ => None,
    }
}

impl App {
    fn create_window(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let attrs = Window::default_attributes()
            .with_title(self.config.title.clone())
            .with_inner_size(PhysicalSize::new(self.config.width, self.config.height));
        let window = Arc::new(el.create_window(attrs).map_err(|e| e.to_string())?);
        let context = Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = Surface::new(&context, window.clone()).map_err(|e| e.to_string())?;
        let s = window.inner_size();
        self.size = (s.width, s.height);
        self.window = Some(window);
        self.surface = Some(surface);
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none()
            && let Err(e) = self.create_window(el)
        {
            self.failure = Some(e);
            self.quit = true;
        }
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.quit = true;
                self.events.push(Event::Quit);
            }
            WindowEvent::Resized(s) => {
                self.size = (s.width, s.height);
                self.events.push(Event::Resize {
                    width: s.width,
                    height: s.height,
                });
            }
            WindowEvent::Focused(f) => {
                self.focused = f;
                if !f {
                    self.events.push(Event::FocusLost);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if !event.repeat => {
                if let PhysicalKey::Code(c) = event.physical_key
                    && let Some(code) = map_key(c)
                {
                    self.events.push(Event::Key {
                        code,
                        pressed: event.state == ElementState::Pressed,
                    });
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = map_button(button) {
                    self.events.push(Event::MouseButton {
                        button,
                        pressed: state == ElementState::Pressed,
                    });
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: DeviceId, event: DeviceEvent) {
        if let DeviceEvent::MouseMotion { delta: (dx, dy) } = event
            && self.focused
        {
            self.events.push(Event::MouseMotion {
                dx: dx as f32,
                dy: dy as f32,
            });
        }
    }
}

/// A winit window with softbuffer presentation.
pub struct DesktopPlatform {
    event_loop: EventLoop<()>,
    app: App,
    start: Instant,
}

impl DesktopPlatform {
    /// Opens the window. An `Err` (for example no display is available)
    /// carries a message for the user.
    pub fn new(config: DesktopConfig) -> Result<Self, String> {
        let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
        let mut p = Self {
            event_loop,
            app: App {
                config,
                window: None,
                surface: None,
                events: Vec::new(),
                size: (1, 1),
                focused: true,
                quit: false,
                failure: None,
            },
            start: Instant::now(),
        };
        // The window is created on the first `resumed`, which may take a
        // few pumps.
        for _ in 0..50 {
            p.pump();
            if p.app.window.is_some() || p.app.failure.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        if let Some(e) = p.app.failure.take() {
            return Err(e);
        }
        if p.app.window.is_none() {
            return Err("the window system never created the window".into());
        }
        p.app.events.clear();
        Ok(p)
    }

    fn pump(&mut self) {
        if let PumpStatus::Exit(_) = self
            .event_loop
            .pump_app_events(Some(Duration::ZERO), &mut self.app)
        {
            self.app.quit = true;
        }
    }
}

impl Platform for DesktopPlatform {
    fn size(&self) -> (u32, u32) {
        self.app.size
    }

    fn present(&mut self, pixels: &[u32], width: u32, height: u32) {
        let (Some(window), Some(surface)) = (&self.app.window, &mut self.app.surface) else {
            return;
        };
        let s = window.inner_size();
        let (Some(w), Some(h)) = (
            std::num::NonZeroU32::new(s.width),
            std::num::NonZeroU32::new(s.height),
        ) else {
            return; // minimized
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        if let Ok(mut buffer) = surface.buffer_mut() {
            blit_fit(pixels, width, height, &mut buffer, s.width, s.height);
            // A failed present is a dropped frame, nothing more.
            let _ = buffer.present();
        }
    }

    fn poll_events(&mut self) -> Vec<Event> {
        self.pump();
        std::mem::take(&mut self.app.events)
    }

    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    fn set_mouse_captured(&mut self, captured: bool) {
        let Some(window) = &self.app.window else {
            return;
        };
        if captured {
            // Locked is unsupported on Windows; Confined keeps the cursor in
            // the window and raw motion still arrives as device events.
            let grabbed = window
                .set_cursor_grab(CursorGrabMode::Locked)
                .or_else(|_| window.set_cursor_grab(CursorGrabMode::Confined));
            window.set_cursor_visible(grabbed.is_err());
        } else {
            // Releasing can fail only if nothing was grabbed.
            let _ = window.set_cursor_grab(CursorGrabMode::None);
            window.set_cursor_visible(true);
        }
    }

    fn should_quit(&self) -> bool {
        self.app.quit
    }
}
