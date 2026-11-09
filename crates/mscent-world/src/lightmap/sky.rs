//! Sky light: the lamps of a vista layer's room, lighting the play space through
//! its viewport faces, as a Source level is lit through its sky faces.
//!
//! A layer shows its room through the map `room = pos + (p - centre) * scale`. A
//! lamp at `L` of radius `r` in the room is, seen from the play space, a lamp at
//! `centre + (L - pos) / scale` of radius `r / scale`: far beyond the windows, and
//! as bright at a play point as the lamp is at that point's image in the room
//! (the falloff depends on `d / r` only). Its shadow ray from a play point leaves
//! the play space through a window, so the part up to the window is traced in
//! the play space and the rest, mapped, in the room. The room's own rock, voxel
//! ranges included, then shades the level, and a play point sees no sun unless a
//! window looks at it.
//!
//! The play lamps go the other way in `mscent-render` (`vista_light.rs`). Both
//! use the map at a layer's `spin` 0.

use mscent_math::epsilon::WINDOW_PROBE_OFFSET;
use mscent_math::{DPlane, Vec3d};

use super::PointLight;

/// A viewport face of the play space, whose plane's normal points into it.
#[derive(Clone, Debug, PartialEq)]
pub struct Window {
    plane: DPlane,
    verts: Vec<Vec3d>,
}

impl Window {
    /// A window of `verts`, counter-clockwise seen from the play space, or `None`
    /// for fewer than three corners or a value that is not finite.
    pub fn new(plane: DPlane, verts: Vec<Vec3d>) -> Option<Self> {
        let finite = plane.normal.is_finite() && plane.dist.is_finite();
        (finite && verts.len() >= 3 && verts.iter().all(|v| v.is_finite()))
            .then_some(Self { plane, verts })
    }

    /// Where the segment `a`..`b` passes through the window from its front, as
    /// the fraction of the way, or `None`.
    fn crossing(&self, a: Vec3d, b: Vec3d) -> Option<f64> {
        let (da, db) = (self.plane.signed_distance(a), self.plane.signed_distance(b));
        if !(da > 0.0 && db < 0.0) {
            return None;
        }
        let t = da / (da - db);
        let at = a + (b - a) * t;
        let n = self.plane.normal;
        let edges = self.verts.iter().zip(self.verts.iter().cycle().skip(1));
        let inside = edges
            .into_iter()
            .all(|(&p, &q)| (q - p).cross(at - p).dot(n) >= 0.0);
        inside.then_some(t)
    }
}

/// The map from the play space to a layer's room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoomMap {
    pub pos: Vec3d,
    pub centre: Vec3d,
    /// The layer's `parallax`, above zero.
    pub scale: f64,
}

impl RoomMap {
    fn in_room(&self, p: Vec3d) -> Vec3d {
        self.pos + (p - self.centre) * self.scale
    }

    fn in_play(&self, q: Vec3d) -> Vec3d {
        self.centre + (q - self.pos) * (1.0 / self.scale)
    }
}

/// One room seen through windows of the play space, and its lamps as the play
/// space sees them.
#[derive(Clone, Debug, PartialEq)]
pub struct SkyRoom {
    map: RoomMap,
    windows: Vec<Window>,
    lamps: Vec<PointLight>,
}

impl SkyRoom {
    /// The room `map` leads to, seen through `windows`, lit by `room_lamps` (in
    /// the room's own coordinates). `None` with no window, a scale that is not
    /// above zero and finite, or no lamp that stays finite once carried out.
    pub fn new(map: RoomMap, windows: Vec<Window>, room_lamps: &[PointLight]) -> Option<Self> {
        let ok = map.scale.is_finite() && map.scale > 0.0;
        if !ok || !map.pos.is_finite() || !map.centre.is_finite() || windows.is_empty() {
            return None;
        }
        let out = |l: &PointLight| {
            let position = map.in_play(l.position.to_f64()).to_f32();
            let radius = (f64::from(l.radius) / map.scale) as f32;
            let usable = position.is_finite() && radius.is_finite() && radius > 0.0;
            usable.then_some(PointLight {
                position,
                radius,
                ..*l
            })
        };
        let lamps: Vec<PointLight> = room_lamps.iter().filter_map(out).collect();
        (!lamps.is_empty()).then_some(Self {
            map,
            windows,
            lamps,
        })
    }

    /// The room's lamps, carried out into the play space.
    pub fn lamps(&self) -> &[PointLight] {
        &self.lamps
    }

    /// How much wider a lamp's disc is seen from the play space than in the room.
    pub(super) fn widen(&self) -> f32 {
        (1.0 / self.map.scale) as f32
    }

    /// Whether the segment from `a` (in the play space) to `b` (beyond a window)
    /// passes a window, with `clear` holding on the play part up to it and on the
    /// room part from its image to `b`'s.
    pub(super) fn sees(&self, a: Vec3d, b: Vec3d, clear: impl Fn(Vec3d, Vec3d) -> bool) -> bool {
        let first = self
            .windows
            .iter()
            .filter_map(|w| w.crossing(a, b).map(|t| (t, w)))
            .min_by(|x, y| x.0.total_cmp(&y.0));
        let Some((t, window)) = first else {
            return false;
        };
        let at = a + (b - a) * t;
        // The window is the face of a solid brush: stop short of it on the play
        // side, and start the room part just past its image.
        let lift = window.plane.normal * WINDOW_PROBE_OFFSET;
        clear(a, at + lift) && clear(self.map.in_room(at - lift), self.map.in_room(b))
    }

    /// Adds the room to a hash: its map, windows and lamps.
    fn hash(&self, h: &mut u64) {
        let mut put = |v: f64| *h = (*h ^ v.to_bits()).wrapping_mul(0x0000_0100_0000_01b3);
        for v in [self.map.pos, self.map.centre] {
            [v.x, v.y, v.z].into_iter().for_each(&mut put);
        }
        put(self.map.scale);
        for w in &self.windows {
            put(w.plane.dist);
            for v in &w.verts {
                [v.x, v.y, v.z].into_iter().for_each(&mut put);
            }
        }
        for l in &self.lamps {
            let (p, c) = (l.position, l.color);
            [p.x, p.y, p.z, c.x, c.y, c.z, l.radius]
                .into_iter()
                .for_each(|v| put(f64::from(v)));
        }
    }
}

/// Every room that lights the play space through its windows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sky {
    rooms: Vec<SkyRoom>,
}

impl Sky {
    pub fn new(rooms: Vec<SkyRoom>) -> Self {
        Self { rooms }
    }

    pub fn rooms(&self) -> &[SkyRoom] {
        &self.rooms
    }

    /// A hash of everything the sky lights with, for a bake's key: 0 for no sky,
    /// so a level without one keeps the key it had.
    pub fn shape_key(&self) -> u64 {
        if self.rooms.is_empty() {
            return 0;
        }
        let mut h = 0xcbf2_9ce4_8422_2325;
        self.rooms.iter().for_each(|r| r.hash(&mut h));
        h
    }
}

#[cfg(test)]
#[path = "sky_tests.rs"]
mod tests;
