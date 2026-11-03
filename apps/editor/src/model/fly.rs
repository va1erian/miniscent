//! The editor's free-fly camera for the 3D pane: WASD to move, the mouse
//! (right button held) or the arrow keys to look, Q/E to roll.
//!
//! It consumes the same [`mscent_platform::Event`]s the game does, so the
//! pane's input path is shared with Instant Play.

use mscent_math::Vec3;
use mscent_platform::{Event, Key};
use mscent_render::View;

use super::document::Document;
use super::view::Eye;

/// Metres (world units) per second at normal speed.
const SPEED: f32 = 8.0;
const FAST: f32 = 3.0;
/// Radians per mouse pixel and per second of arrow key.
const MOUSE: f32 = 0.0035;
const TURN: f32 = 1.6;
const MAX_PITCH: f32 = 1.55;

#[derive(Clone, Debug, PartialEq)]
pub struct FlyCam {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    held: Vec<Key>,
}

impl FlyCam {
    pub fn new(pos: Vec3, yaw: f32, pitch: f32) -> FlyCam {
        FlyCam {
            pos,
            yaw,
            pitch,
            roll: 0.0,
            held: Vec::new(),
        }
    }

    /// A camera for `doc`: at the player start when there is one, else
    /// outside the level's corner looking at its middle.
    pub fn for_document(doc: &Document) -> FlyCam {
        use mscent_world::Entity;
        let start = doc.entities().iter().find_map(|e| match e.entity {
            Entity::PlayerStart { pos, yaw } => Some((pos, yaw)),
            _ => None,
        });
        if let Some((pos, yaw)) = start {
            return FlyCam::new(pos, yaw, 0.0);
        }
        let (lo, hi) = doc
            .bounds()
            .unwrap_or(([-8.0, -8.0, -2.0], [8.0, 8.0, 6.0]));
        let mid = |i: usize| ((lo[i] + hi[i]) * 0.5) as f32;
        let span = (0..3).map(|i| (hi[i] - lo[i]) as f32).fold(4.0, f32::max);
        FlyCam::new(
            Vec3::new(mid(0), lo[1] as f32 - span * 0.3, mid(2) + span * 0.3),
            0.0,
            -0.3,
        )
    }

    pub fn view(&self) -> View {
        View::from_angles(self.pos, self.yaw, self.pitch, self.roll)
    }

    /// Where the camera is and looks, for the 2D views.
    pub fn eye(&self) -> Eye {
        let (p, f) = (self.pos, self.view().forward());
        Eye {
            pos: [p.x, p.y, p.z].map(f64::from),
            forward: [f.x, f.y, f.z].map(f64::from),
        }
    }

    fn down(&self, c: char) -> bool {
        self.held.contains(&Key::Char(c))
    }

    /// Feeds one input event. Returns whether the picture will change.
    pub fn event(&mut self, e: &Event) -> bool {
        match *e {
            Event::Key { code, pressed } => {
                self.held.retain(|k| *k != code);
                if pressed {
                    self.held.push(code);
                }
                false
            }
            Event::MouseMotion { dx, dy } if dx.is_finite() && dy.is_finite() => {
                self.yaw -= dx * MOUSE;
                self.pitch = (self.pitch - dy * MOUSE).clamp(-MAX_PITCH, MAX_PITCH);
                true
            }
            Event::FocusLost => {
                self.held.clear();
                false
            }
            _ => false,
        }
    }

    /// Whether any movement key is held.
    pub fn moving(&self) -> bool {
        self.held.iter().any(|k| {
            matches!(
                k,
                Key::Char('W' | 'A' | 'S' | 'D' | 'R' | 'F' | 'Q' | 'E')
                    | Key::Left
                    | Key::Right
                    | Key::Up
                    | Key::Down
            )
        })
    }

    /// Advances by `dt` seconds of held keys. Returns whether the pose changed.
    pub fn step(&mut self, dt: f32) -> bool {
        if !self.moving() || !dt.is_finite() {
            return false;
        }
        let axis = |plus: bool, minus: bool| f32::from(u8::from(plus)) - f32::from(u8::from(minus));
        let held = |k| self.held.contains(&k);
        self.yaw += axis(held(Key::Left), held(Key::Right)) * TURN * dt;
        self.pitch = (self.pitch + axis(held(Key::Up), held(Key::Down)) * TURN * dt)
            .clamp(-MAX_PITCH, MAX_PITCH);
        self.roll += axis(self.down('Q'), self.down('E')) * TURN * dt;
        let speed = SPEED * if held(Key::Shift) { FAST } else { 1.0 } * dt;
        let o = self.view().orientation;
        let step = o.rotate(Vec3::new(1.0, 0.0, 0.0))
            * (axis(self.down('D'), self.down('A')) * speed)
            + o.rotate(Vec3::new(0.0, 0.0, -1.0)) * (axis(self.down('W'), self.down('S')) * speed)
            + Vec3::new(0.0, 0.0, axis(self.down('R'), self.down('F')) * speed);
        if step.is_finite() {
            self.pos = self.pos + step;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eye_is_where_the_camera_is_and_looks_where_it_looks() {
        let cam = FlyCam::new(Vec3::new(1.0, 2.0, 3.0), 0.0, 0.0);
        let eye = cam.eye();
        assert_eq!(eye.pos, [1.0, 2.0, 3.0]);
        // Yaw 0 looks along +Y; a quarter turn left looks along -X.
        let near = |a: [f64; 3], b: [f64; 3]| (0..3).all(|i| (a[i] - b[i]).abs() < 1e-5);
        assert!(near(eye.forward, [0.0, 1.0, 0.0]), "{:?}", eye.forward);
        let turned = FlyCam::new(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0).eye();
        assert!(
            near(turned.forward, [-1.0, 0.0, 0.0]),
            "{:?}",
            turned.forward
        );
    }

    fn key(c: char, pressed: bool) -> Event {
        Event::Key {
            code: Key::Char(c),
            pressed,
        }
    }

    #[test]
    fn w_moves_along_the_view_and_release_stops() {
        let mut cam = FlyCam::new(Vec3::ZERO, 0.0, 0.0);
        cam.event(&key('W', true));
        assert!(cam.step(0.5));
        assert!(
            (cam.pos.y - 4.0).abs() < 1e-3,
            "yaw 0 looks along +Y: {:?}",
            cam.pos
        );
        cam.event(&key('W', false));
        assert!(!cam.step(0.5));
    }

    #[test]
    fn mouse_look_turns_and_clamps_pitch() {
        let mut cam = FlyCam::new(Vec3::ZERO, 0.0, 0.0);
        cam.event(&Event::MouseMotion { dx: 100.0, dy: 0.0 });
        assert!(cam.yaw < 0.0, "moving right turns right");
        cam.event(&Event::MouseMotion {
            dx: 0.0,
            dy: -1.0e6,
        });
        assert!(cam.pitch <= MAX_PITCH);
        cam.event(&Event::MouseMotion {
            dx: f32::NAN,
            dy: 0.0,
        });
        assert!(cam.yaw.is_finite());
    }

    #[test]
    fn roll_changes_the_up_vector_not_the_heading() {
        let mut cam = FlyCam::new(Vec3::ZERO, 0.0, 0.0);
        let before = cam.view().forward();
        cam.event(&key('Q', true));
        cam.step(0.5);
        let after = cam.view().forward();
        assert!((before - after).length() < 1e-4);
        assert!(cam.roll > 0.5);
    }
}
