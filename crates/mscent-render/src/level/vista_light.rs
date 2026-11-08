//! The play space's lamps, carried into the rooms of vista layers, so that a
//! miniature continuing the level is lit as the level is where the two meet.
//!
//! A play lamp at `p` of radius `r` lights a layer's room as a lamp at
//! `pos + (p - centre) * parallax` of radius `r * parallax`: the image of the lamp
//! under the same map that gives the layer its eye. The baker's falloff,
//! `(1 - d / r)^3` times the cosine, depends on `d / r` and on directions only, so
//! a point of the room receives from the image what its twin in the play space
//! receives from the lamp. Floors, walls and voxels that continue the level
//! across a viewport face therefore meet at the same brightness, and the baked
//! shadows and bounce of the room follow as they would for any of its own lamps.
//!
//! Only a lamp that shines on a viewport face of the layer's slot is carried: one
//! behind the viewport wall, out of range of it or hidden from it by the level
//! lights nothing that face shows. The test is per lamp, not per ray, so a lamp
//! that sees part of a window also lights what the frame around it would hide.
//! An infinitely far layer (`parallax` 0) gets nothing: no lamp reaches it.

use mscent_math::epsilon::WINDOW_PROBE_OFFSET;
use mscent_math::{Vec3, Vec3d};
use mscent_world::lightmap::PointLight;
use mscent_world::lightmap::ray::segment_clear;
use mscent_world::{Bsp, Face};

use super::{Layer, Vistas};

impl Vistas {
    /// `lights` followed by the images of its play-space lamps in each layer's
    /// room. A room shared by layers takes the map of the first, so that no lamp
    /// lights it twice; the others' slots still count as its windows.
    pub(in crate::level) fn with_carried(
        &self,
        bsp: &Bsp,
        holes: &[(Face, u8)],
        mut lights: Vec<PointLight>,
    ) -> Vec<PointLight> {
        let own = lights.len();
        for (index, layer) in self.layers.iter().enumerate() {
            if !self.carries_into(index) {
                continue;
            }
            let slots = self.slots_of(layer.island);
            let windows: Vec<&Face> = (holes.iter())
                .filter(|(_, slot)| slots.contains(slot))
                .map(|(face, _)| face)
                .collect();
            for i in 0..own {
                let lamp = lights[i];
                if !self.in_play_space(bsp, lamp.position) {
                    continue;
                }
                if !windows.iter().any(|w| shines_on(bsp, &lamp, w)) {
                    continue;
                }
                if let Some(image) = self.image_in(bsp, layer, &lamp) {
                    lights.push(image);
                }
            }
        }
        lights
    }

    /// Whether layer `index` is the first of its room with a finite scale, in a
    /// room of its own: the one whose map the room is lit by.
    fn carries_into(&self, index: usize) -> bool {
        let Some(layer) = self.layers.get(index) else {
            return false;
        };
        let lit = |l: &Layer| l.parallax > 0.0 && l.island == layer.island;
        let first = self.layers.iter().position(lit);
        first == Some(index) && self.remote_island(layer.island)
    }

    /// The slots that show the room `island`.
    fn slots_of(&self, island: u32) -> Vec<u8> {
        let mut slots: Vec<u8> = (self.layers.iter())
            .filter(|l| l.island == island)
            .map(|l| l.slot)
            .collect();
        slots.dedup();
        slots
    }

    /// Whether `pos` is in an empty leaf outside every layer's room. A lamp in
    /// solid lights nothing of the play space, so its image must not either.
    fn in_play_space(&self, bsp: &Bsp, pos: Vec3) -> bool {
        let leaf = bsp.leaf_at_f32(pos);
        self.islands.island_of(leaf).is_some() && !self.in_remote(leaf)
    }

    fn remote_island(&self, island: u32) -> bool {
        (self.remote.binary_search_by_key(&island, |&(i, _)| i)).is_ok()
    }

    /// The lamp as `layer`'s room sees it, or `None` when its image falls outside
    /// the room, where it would light some other place or nothing.
    fn image_in(&self, bsp: &Bsp, layer: &Layer, lamp: &PointLight) -> Option<PointLight> {
        let position = layer.pos + (lamp.position - layer.centre) * layer.parallax;
        let radius = lamp.radius * layer.parallax;
        let usable = position.is_finite() && radius.is_finite() && radius > 0.0;
        let leaf = usable.then(|| bsp.leaf_at_f32(position))?;
        (self.islands.island_of(leaf) == Some(layer.island)).then_some(PointLight {
            position,
            radius,
            ..*lamp
        })
    }
}

/// Whether `lamp` lights some of `window`: a point of it within range, in front
/// of it, in the lamp's cone and in clear sight. The points tried are the nearest
/// to the lamp, the middle, and each corner drawn a little towards the middle.
fn shines_on(bsp: &Bsp, lamp: &PointLight, window: &Face) -> bool {
    let from = lamp.position.to_f64();
    if window.plane.signed_distance(from) <= WINDOW_PROBE_OFFSET {
        return false;
    }
    let verts = window.polygon.vertices();
    let Some(middle) = centroid(verts) else {
        return false;
    };
    let lift = window.plane.normal * WINDOW_PROBE_OFFSET;
    let inset = verts.iter().map(|&v| v.lerp(middle, 0.01));
    let mut probes = [nearest_on(window, from), middle].into_iter().chain(inset);
    probes.any(|p| {
        let at = p + lift;
        let to = (at - from).to_f32();
        let in_cone = match &lamp.cone {
            Some(cone) => to.normalized().is_some_and(|d| !cone.misses(d)),
            None => true,
        };
        to.length() < lamp.radius && in_cone && segment_clear(bsp, from, at)
    })
}

fn centroid(verts: &[Vec3d]) -> Option<Vec3d> {
    let n = verts.len();
    let sum = verts.iter().fold(Vec3d::ZERO, |s, &v| s + v);
    (n > 0).then(|| sum * (1.0 / n as f64))
}

/// The point of the convex `face` nearest to `p`.
fn nearest_on(face: &Face, p: Vec3d) -> Vec3d {
    let n = face.plane.normal;
    let on_plane = p - n * face.plane.signed_distance(p);
    let verts = face.polygon.vertices();
    let edges = || verts.iter().zip(verts.iter().cycle().skip(1));
    // Counter-clockwise seen from the front, so inside is left of every edge.
    let inside = edges().all(|(&a, &b)| (b - a).cross(on_plane - a).dot(n) >= 0.0);
    if inside {
        return on_plane;
    }
    let on_edge = |(&a, &b): (&Vec3d, &Vec3d)| {
        let ab = b - a;
        let len2 = ab.length_squared();
        let t = if len2 > 0.0 {
            ((on_plane - a).dot(ab) / len2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        a + ab * t
    };
    let dist = |q: &Vec3d| (*q - p).length_squared();
    edges()
        .map(on_edge)
        .min_by(|a, b| dist(a).total_cmp(&dist(b)))
        .unwrap_or(on_plane)
}
