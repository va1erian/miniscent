//! Portal flood: which empty leaves can the camera see?
//!
//! Starting at the camera's leaf, the flood crosses a portal only if the
//! camera is on the correct side of it and the portal's projection overlaps
//! the screen window that survived the portals crossed so far. The window
//! narrows at each step, which is the frustum clipping of the classic portal
//! renderer, done on screen rectangles instead of plane sets.
//!
//! The result is a conservative superset of the truly visible leaves: the
//! z-buffer makes extra leaves harmless, missing ones would not be.

use mscent_math::Vec3;
use mscent_math::epsilon::PORTAL_CLIP_DEPTH;
use mscent_world::LeafId;

use super::{SIDE_SLACK, Scene};
use crate::camera::Camera;

/// Slack, in pixels, added around projected portals so rounding never
/// closes a doorway that is visible.
const MARGIN: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Rect {
    fn intersect(self, o: Rect) -> Option<Rect> {
        let r = Rect {
            x0: self.x0.max(o.x0),
            y0: self.y0.max(o.y0),
            x1: self.x1.min(o.x1),
            y1: self.y1.min(o.y1),
        };
        (r.x0 < r.x1 && r.y0 < r.y1).then_some(r)
    }

    fn union(self, o: Rect) -> Rect {
        Rect {
            x0: self.x0.min(o.x0),
            y0: self.y0.min(o.y0),
            x1: self.x1.max(o.x1),
            y1: self.y1.max(o.y1),
        }
    }

    fn contains(&self, o: &Rect) -> bool {
        self.x0 <= o.x0 && self.y0 <= o.y0 && self.x1 >= o.x1 && self.y1 >= o.y1
    }
}

/// Visibility flags per leaf, indexed by leaf id.
pub(super) fn visible_leaves(scene: &Scene, cam: &Camera, eye: Vec3, start: LeafId) -> Vec<bool> {
    let screen = [0.0, 0.0, cam.width() as f32, cam.height() as f32];
    visible_through(scene, cam, eye, start, screen)
}

/// [`visible_leaves`] through the screen window `[x0, y0, x1, y1]`.
pub(super) fn visible_through(
    scene: &Scene,
    cam: &Camera,
    eye: Vec3,
    start: LeafId,
    [x0, y0, x1, y1]: [f32; 4],
) -> Vec<bool> {
    let leaf_count = scene.bsp.leaves().len();
    let mut seen: Vec<Option<Rect>> = vec![None; leaf_count];
    let screen = Rect { x0, y0, x1, y1 };
    let Some(slot) = seen.get_mut(start.0 as usize) else {
        return vec![true; leaf_count];
    };
    *slot = Some(screen);
    let mut work = vec![(start, screen)];
    // Rectangles only ever grow and take values from a finite set, so the
    // flood ends on its own; the budget guards against pathological input.
    let mut budget = 64 * scene.portals.portals().len() + 1024;
    while let Some((leaf, window)) = work.pop() {
        for &pi in scene.portals.leaf_portals(leaf) {
            budget = budget.saturating_sub(1);
            if budget == 0 {
                return vec![true; leaf_count];
            }
            let Some((target, rect)) = cross(scene, cam, eye, leaf, pi as usize, window) else {
                continue;
            };
            let Some(entry) = seen.get_mut(target.0 as usize) else {
                continue;
            };
            if entry.is_some_and(|old| old.contains(&rect)) {
                continue;
            }
            // The leaf floods on with all it has been seen through, not only this
            // window: windows that came before may have bounded a part of the
            // screen that none of them looked through, and the next window there
            // would be skipped as seen. Wider only adds leaves, which is safe.
            let grown = entry.map_or(rect, |old| old.union(rect));
            *entry = Some(grown);
            work.push((target, grown));
        }
    }
    seen.iter().map(Option::is_some).collect()
}

/// The leaf beyond portal `pi` and the narrowed window, or `None` if the
/// portal cannot be seen through.
fn cross(
    scene: &Scene,
    cam: &Camera,
    eye: Vec3,
    from: LeafId,
    pi: usize,
    window: Rect,
) -> Option<(LeafId, Rect)> {
    let portal = scene.portals.portals().get(pi)?;
    let target = portal.other(from)?;
    let info = scene.portal_info.get(pi)?;
    let side = info.plane.signed_distance(eye.to_f64());
    // Only the side the camera is on may be looked through.
    let facing = if from == portal.front { side } else { -side };
    if facing < -SIDE_SLACK {
        return None;
    }
    // Eye (nearly) in the portal plane: the projection degenerates, so pass
    // the window through unchanged. Over-inclusion is safe.
    if side.abs() < f64::from(cam.near()).max(0.05) {
        return Some((target, window));
    }
    let rect = project_rect(cam, &info.points)?;
    Some((target, window.intersect(rect)?))
}

/// Screen bounds of the part of a world-space polygon in front of the eye. Not
/// the part past the near plane: a ray that crosses a doorway nearer than that
/// still sees the room beyond it, which an eye just off the doorway's plane does
/// across much of the screen.
fn project_rect(cam: &Camera, points: &[Vec3]) -> Option<Rect> {
    let cam_pts: Vec<Vec3> = points.iter().map(|&p| cam.world_to_camera(p)).collect();
    let mut rect: Option<Rect> = None;
    for p in clip_near(&cam_pts, PORTAL_CLIP_DEPTH) {
        let s = cam.project(p)?;
        let r = Rect {
            x0: s.x - MARGIN,
            y0: s.y - MARGIN,
            x1: s.x + MARGIN,
            y1: s.y + MARGIN,
        };
        rect = Some(rect.map_or(r, |acc| acc.union(r)));
    }
    rect
}

/// Keeps the part of the polygon at least `near` in front of the eye.
fn clip_near(pts: &[Vec3], near: f32) -> Vec<Vec3> {
    let depth = |p: Vec3| -p.z - near;
    let mut out = Vec::with_capacity(pts.len() + 1);
    let Some(&last) = pts.last() else { return out };
    let mut prev = last;
    for &cur in pts {
        let (dp, dc) = (depth(prev), depth(cur));
        if (dp >= 0.0) != (dc >= 0.0) {
            out.push(prev.lerp(cur, dp / (dp - dc)));
        }
        if dc >= 0.0 {
            out.push(cur);
        }
        prev = cur;
    }
    out
}
