//! Device state to player intent: which keys are held, how far the mouse
//! moved this frame.

use mscent_game::Input;
use mscent_math::Vec3;
use mscent_platform::{Event, Key, MouseButton};

/// Held keys and the mouse movement gathered since the last frame.
#[derive(Clone, Debug, Default)]
pub struct Controls {
    held: Vec<Key>,
    fire_button: bool,
    mouse: (f32, f32),
}

/// A key press the menus or the session care about, reported once.
pub type Press = Key;

impl Controls {
    /// Records one platform event. Returns the key when it was a fresh press
    /// (for menu navigation and one-shot actions).
    pub fn event(&mut self, e: &Event) -> Option<Press> {
        match *e {
            Event::Key { code, pressed } => {
                let was_held = self.held.contains(&code);
                self.held.retain(|k| *k != code);
                if pressed {
                    self.held.push(code);
                    return (!was_held).then_some(code);
                }
            }
            Event::MouseButton {
                button: MouseButton::Left,
                pressed,
            } => self.fire_button = pressed,
            Event::MouseMotion { dx, dy } if dx.is_finite() && dy.is_finite() => {
                self.mouse.0 += dx;
                self.mouse.1 += dy;
            }
            Event::FocusLost => self.release_all(),
            _ => {}
        }
        None
    }

    /// Drops every held key and button, e.g. when the window loses focus.
    pub fn release_all(&mut self) {
        self.held.clear();
        self.fire_button = false;
        self.mouse = (0.0, 0.0);
    }

    fn down(&self, c: char) -> bool {
        self.held.contains(&Key::Char(c))
    }

    fn axis(&self, plus: bool, minus: bool) -> f32 {
        f32::from(u8::from(plus)) - f32::from(u8::from(minus))
    }

    /// Whether the fire control is held: the left button, or Space in the ship
    /// (on foot Space jumps, as in a Quake-like).
    pub fn firing(&self, on_foot: bool) -> bool {
        self.fire_button || (!on_foot && self.held.contains(&Key::Space))
    }

    /// Takes the mouse movement accumulated since the last call.
    pub fn take_mouse(&mut self) -> (f32, f32) {
        std::mem::take(&mut self.mouse)
    }

    /// The thrust, key-turn, roll and fire parts of the player's intent.
    /// Mouse look is applied separately, directly to the orientation. On foot,
    /// WASD walks and Space or R jumps (held in the air: the jetpack). B leaves
    /// a landed ship or boards it again.
    pub fn input(&self, on_foot: bool) -> Input {
        let held = |k| self.held.contains(&k);
        let up = self.down('R') || (on_foot && held(Key::Space));
        Input {
            thrust: Vec3::new(
                self.axis(self.down('D'), self.down('A')),
                self.axis(up, self.down('F')),
                self.axis(self.down('W'), self.down('S')),
            ),
            pitch: self.axis(held(Key::Up), held(Key::Down)),
            yaw: self.axis(held(Key::Left), held(Key::Right)),
            roll: self.axis(self.down('Q'), self.down('E')),
            fire: self.firing(on_foot),
            use_key: self.down('G'),
            board: self.down('B'),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char, pressed: bool) -> Event {
        Event::Key {
            code: Key::Char(c),
            pressed,
        }
    }

    #[test]
    fn wasd_maps_to_thrust_and_release_clears_it() {
        let mut c = Controls::default();
        assert_eq!(c.event(&key('W', true)), Some(Key::Char('W')));
        assert_eq!(
            c.event(&key('W', true)),
            None,
            "a repeat is not a new press"
        );
        c.event(&key('D', true));
        let i = c.input(false);
        assert_eq!((i.thrust.x, i.thrust.z), (1.0, 1.0));
        c.event(&key('W', false));
        assert_eq!(c.input(false).thrust.z, 0.0);
        c.event(&Event::FocusLost);
        assert_eq!(c.input(false).thrust, Vec3::ZERO);
    }

    #[test]
    fn mouse_accumulates_and_is_taken_once() {
        let mut c = Controls::default();
        c.event(&Event::MouseMotion { dx: 3.0, dy: -1.0 });
        c.event(&Event::MouseMotion { dx: 2.0, dy: 0.0 });
        c.event(&Event::MouseMotion {
            dx: f32::NAN,
            dy: 1.0,
        });
        assert_eq!(c.take_mouse(), (5.0, -1.0));
        assert_eq!(c.take_mouse(), (0.0, 0.0));
    }

    #[test]
    fn fire_comes_from_the_left_button_or_space() {
        let mut c = Controls::default();
        c.event(&Event::MouseButton {
            button: MouseButton::Left,
            pressed: true,
        });
        assert!(c.input(false).fire);
        c.event(&Event::MouseButton {
            button: MouseButton::Left,
            pressed: false,
        });
        assert!(!c.input(false).fire);
        c.event(&Event::Key {
            code: Key::Space,
            pressed: true,
        });
        assert!(c.input(false).fire);
    }
}

#[cfg(test)]
mod foot_tests {
    use super::*;

    #[test]
    fn on_foot_space_jumps_and_b_boards() {
        let mut c = Controls::default();
        c.event(&Event::Key {
            code: Key::Space,
            pressed: true,
        });
        let walking = c.input(true);
        assert_eq!(walking.thrust.y, 1.0);
        assert!(!walking.fire, "Space jumps on foot, it does not fire");
        assert!(c.input(false).fire, "in the ship Space fires");
        c.event(&Event::Key {
            code: Key::Char('B'),
            pressed: true,
        });
        assert!(c.input(false).board && c.input(true).board);
    }
}
