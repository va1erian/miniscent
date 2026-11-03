//! Entities drawn into the scene and the 2D heads-up display.

use mscent_math::Vec3;

use crate::camera::Camera;
use crate::draw2d::{draw_text, fill_rect, stroke_rect};
use crate::dynlight::DynLight;
use crate::framebuffer::Framebuffer;
use crate::mesh::{Mesh, MeshLighting, Pose, each_lit_triangle, fill_meshes, list_mesh};
use crate::raster::PolyList;
use crate::scene::Scene;
use crate::shade::Fog;
use crate::texture::Texture;

/// What kind of thing an [`Actor`] is, which picks its mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorKind {
    Enemy,
    Goal,
    /// An enemy on the player's side.
    Friendly,
    Switch,
    Keycard,
    Terminal,
    Crate,
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

/// What lights actors beyond the level's own lights: the lights of the frame
/// and a copy of the level's lighting to add them to, whose first `base`
/// points are the level's.
pub(super) struct LitActors<'a> {
    pub scene: &'a Scene,
    pub lights: &'a [DynLight],
    pub lighting: &'a mut MeshLighting,
    pub base: usize,
}

impl LitActors<'_> {
    /// The lighting of `a`: the level's, and the lights that reach the leaf it
    /// stands in, so a flash behind a wall does not light it.
    fn of(&mut self, a: &Actor) -> &MeshLighting {
        self.lighting.points.truncate(self.base);
        let leaf = self.scene.bsp().leaf_at_f32(a.pos);
        let reaching = self
            .lights
            .iter()
            .filter(|l| l.is_lit() && l.reaches_leaf(leaf))
            .map(DynLight::mesh_light);
        self.lighting.points.extend(reaching);
        self.lighting
    }
}

fn pose_of(a: &Actor) -> Pose {
    Pose {
        pos: a.pos,
        yaw: a.yaw,
        scale: ACTOR_SCALE,
    }
}

/// Meshes for the actor kinds, built once.
pub(super) struct ActorMeshes {
    enemy: Mesh,
    goal: Mesh,
    friendly: Mesh,
    switch: Mesh,
    keycard: Mesh,
    terminal: Mesh,
    crate_box: Mesh,
    /// Carries the vertex light in its `u` coordinate; its texel is unused.
    carrier: Texture,
}

impl ActorMeshes {
    pub(super) fn new() -> Self {
        Self {
            enemy: Mesh::enemy(),
            goal: Mesh::goal(),
            friendly: Mesh::friendly(),
            switch: Mesh::switch(),
            keycard: Mesh::keycard(),
            terminal: Mesh::terminal(),
            crate_box: Mesh::crate_box(),
            carrier: Texture::solid(0xffff_ffff),
        }
    }

    fn mesh_of(&self, kind: ActorKind) -> &Mesh {
        match kind {
            ActorKind::Enemy => &self.enemy,
            ActorKind::Goal => &self.goal,
            ActorKind::Friendly => &self.friendly,
            ActorKind::Switch => &self.switch,
            ActorKind::Keycard => &self.keycard,
            ActorKind::Terminal => &self.terminal,
            ActorKind::Crate => &self.crate_box,
        }
    }

    /// Each triangle of `actors` that faces `eye`, lit by `lighting`: its corners
    /// in world space with their light factors, and its flat colour.
    pub(super) fn each_triangle(
        &self,
        actors: &[Actor],
        lighting: &MeshLighting,
        eye: Vec3,
        mut visit: impl FnMut([(Vec3, f32); 3], u32),
    ) {
        for a in actors {
            let pose = pose_of(a);
            each_lit_triangle(self.mesh_of(a.kind), &pose, lighting, eye, &mut visit);
        }
    }

    /// Like [`Self::each_triangle`], with each actor also lit by the dynamic
    /// lights of `lit`, as [`Self::list_lit`] lights it.
    pub(super) fn each_triangle_lit(
        &self,
        actors: &[Actor],
        mut lit: LitActors,
        eye: Vec3,
        mut visit: impl FnMut([(Vec3, f32); 3], u32),
    ) {
        for a in actors {
            let pose = pose_of(a);
            let lighting = lit.of(a);
            each_lit_triangle(self.mesh_of(a.kind), &pose, lighting, eye, &mut visit);
        }
    }

    /// Replaces `list` with this frame's actor triangles.
    pub(super) fn list(
        &self,
        cam: &Camera,
        actors: &[Actor],
        lighting: &MeshLighting,
        list: &mut PolyList,
    ) {
        list.clear();
        for a in actors {
            let pose = pose_of(a);
            list_mesh(
                cam,
                self.mesh_of(a.kind),
                &pose,
                lighting,
                &self.carrier,
                list,
            );
        }
    }

    /// Like [`Self::list`], with each actor also lit by the dynamic lights
    /// that reach the leaf it stands in, so a flash behind a wall does not
    /// light it.
    pub(super) fn list_lit(
        &self,
        cam: &Camera,
        actors: &[Actor],
        lit: LitActors,
        list: &mut PolyList,
    ) {
        list.clear();
        let mut lit = lit;
        for a in actors {
            let pose = pose_of(a);
            let lighting = lit.of(a);
            list_mesh(
                cam,
                self.mesh_of(a.kind),
                &pose,
                lighting,
                &self.carrier,
                list,
            );
        }
    }

    /// Fills the listed triangles into the rows `fb` holds.
    pub(super) fn fill(&self, fb: &mut Framebuffer, list: &PolyList, fog: Option<Fog>) {
        fill_meshes(fb, list, &self.carrier, fog);
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
