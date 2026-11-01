//! Entities drawn into the scene and the 2D heads-up display.

use mscent_math::Vec3;

use crate::camera::Camera;
use crate::draw2d::{draw_text, fill_rect, stroke_rect};
use crate::framebuffer::Framebuffer;
use crate::mesh::{Mesh, MeshLighting, Pose, draw_mesh};
use crate::shade::Fog;

/// What kind of thing an [`Actor`] is, which picks its mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorKind {
    Enemy,
    Goal,
}

/// A mesh placed in the level. `yaw` turns it about +Z.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Actor {
    pub kind: ActorKind,
    pub pos: Vec3,
    pub yaw: f32,
}

/// Meshes are modelled about a unit wide; this makes them read at a distance.
const ACTOR_SCALE: f32 = 1.5;

/// Meshes for the actor kinds, built once.
pub(super) struct ActorMeshes {
    enemy: Mesh,
    goal: Mesh,
}

impl ActorMeshes {
    pub(super) fn new() -> Self {
        Self {
            enemy: Mesh::enemy(),
            goal: Mesh::goal(),
        }
    }

    pub(super) fn draw(
        &self,
        fb: &mut Framebuffer,
        cam: &Camera,
        actors: &[Actor],
        lighting: &MeshLighting,
        fog: Option<Fog>,
    ) {
        for a in actors {
            let mesh = match a.kind {
                ActorKind::Enemy => &self.enemy,
                ActorKind::Goal => &self.goal,
            };
            let pose = Pose {
                pos: a.pos,
                yaw: a.yaw,
                scale: ACTOR_SCALE,
            };
            draw_mesh(fb, cam, mesh, &pose, lighting, fog);
        }
    }
}

/// Numbers shown by the heads-up display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Overlay {
    pub health: i32,
    pub max_health: i32,
    pub fps: f32,
}

const WHITE: u32 = 0xffff_ffff;
const SHADOW: u32 = 0xff00_0000;

/// Text with a one-pixel drop shadow so it reads on any background.
fn label(fb: &mut Framebuffer, x: i32, y: i32, text: &str, color: u32) {
    draw_text(fb, x + 1, y + 1, text, SHADOW, 1);
    draw_text(fb, x, y, text, color, 1);
}

/// Health bar and text at the bottom left, frame rate at the top right.
pub(super) fn draw_overlay(fb: &mut Framebuffer, o: &Overlay) {
    let (w, h) = (fb.width() as i32, fb.height() as i32);
    let max = o.max_health.max(1);
    let hp = o.health.clamp(0, max);
    let color = match i64::from(hp) * 4 / i64::from(max) {
        0 => 0xffe0_3030,
        1 => 0xffe0_c030,
        _ => 0xff30_d060,
    };
    let (bx, by, bw, bh) = (6, h - 14, 60u32, 7u32);
    fill_rect(fb, bx, by, bw, bh, 0xff20_2020);
    fill_rect(
        fb,
        bx,
        by,
        (u64::from(bw) * hp as u64 / max as u64) as u32,
        bh,
        color,
    );
    stroke_rect(fb, bx - 1, by - 1, bw + 2, bh + 2, 1, 0xffc0_c0c0);
    label(fb, bx + bw as i32 + 6, by, &format!("HP {hp}"), WHITE);
    let fps = format!("{:.0} FPS", o.fps);
    let (tw, _) = crate::draw2d::text_size(&fps, 1);
    label(fb, w - tw as i32 - 6, 5, &fps, WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn huge_health_values_do_not_overflow_the_bar() {
        let mut fb = Framebuffer::new(160, 90).unwrap();
        for (health, max_health) in [(i32::MAX, i32::MAX), (i32::MAX / 2, i32::MAX), (5, 0)] {
            let o = Overlay {
                health,
                max_health,
                fps: f32::NAN,
            };
            draw_overlay(&mut fb, &o);
        }
        // A full bar fills its whole width.
        let o = Overlay {
            health: i32::MAX,
            max_health: i32::MAX,
            fps: 60.0,
        };
        draw_overlay(&mut fb, &o);
        let full = fb.pixels()[(fb.height() as usize - 11) * fb.width() as usize + 6 + 59];
        assert_eq!(full, 0xff30_d060);
    }
}
