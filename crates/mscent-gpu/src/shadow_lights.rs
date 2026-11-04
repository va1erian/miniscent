//! Which lights cast shadows, and the faces of the cube each is drawn into.
//!
//! A frame has up to `MAX_DYN_LIGHTS` lights and room for `SHADOW_LIGHTS` maps,
//! so the lights that matter most get them: the brightest over their distance to
//! the eye. Two lights near a tie would swap their shadows frame to frame, which
//! is a flicker, so a light keeps its map until another is clearly stronger. The
//! level's static lamps choose among their own maps the same way (`lamp_shadows.rs`).

use mscent_math::Vec3;
use mscent_render::{DynLight, MAX_DYN_LIGHTS};

use crate::dyn_lights::used;

/// Lights that cast shadows in one frame.
pub(crate) const SHADOW_LIGHTS: usize = 2;
/// A challenger takes a map from the light that has it only by being this much
/// stronger.
const HYSTERESIS: f32 = 1.25;
/// What `Frame` holds for a light with no map.
pub(crate) const NO_MAP: u32 = u32::MAX;

/// By light of a frame, which map it has, as `shadow_slots` of `Frame`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slots(pub(crate) [u32; MAX_DYN_LIGHTS]);

impl Slots {
    /// No light has a map.
    pub(crate) const NONE: Self = Self([NO_MAP; MAX_DYN_LIGHTS]);

    pub(crate) fn push(self, bytes: &mut Vec<u8>) {
        for slot in self.0 {
            bytes.extend_from_slice(&slot.to_le_bytes());
        }
    }
}

/// What picking a light's map needs of it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Candidate {
    pub(crate) pos: Vec3,
    pub(crate) radius: f32,
    pub(crate) brightness: f32,
}

impl Candidate {
    /// `light` seen from `eye`, or `None` for a light that is dark or not a
    /// place, which casts nothing.
    pub(crate) fn of(light: &DynLight, eye: Vec3) -> Option<Self> {
        let c = light.color();
        let mean = (c.x + c.y + c.z) / 3.0;
        Self::new(light.pos(), light.radius(), mean, eye).filter(|_| light.is_lit())
    }

    /// A light at `pos` of `radius` and a colour of mean `mean`, from `eye`. A
    /// value that is not finite, or a radius or brightness that is not above zero,
    /// is no light.
    pub(crate) fn new(pos: Vec3, radius: f32, mean: f32, eye: Vec3) -> Option<Self> {
        let brightness = mean / (pos - eye).length().max(1.0);
        let usable = pos.is_finite() && eye.is_finite() && radius.is_finite() && radius > 0.0;
        (usable && brightness.is_finite() && brightness > 0.0).then_some(Self {
            pos,
            radius,
            brightness,
        })
    }
}

/// Who holds each of `N` maps, kept from frame to frame.
pub(crate) struct Pick<const N: usize> {
    held: [Option<usize>; N],
}

impl<const N: usize> Default for Pick<N> {
    fn default() -> Self {
        Self { held: [None; N] }
    }
}

impl<const N: usize> Pick<N> {
    /// Gives the maps to the strongest of `candidates`, by light index, and
    /// returns which light holds each.
    pub(crate) fn choose(&mut self, candidates: &[Option<Candidate>]) -> [Option<usize>; N] {
        let strength = |i: usize| candidates.get(i).copied().flatten().map(|c| c.brightness);
        for holder in &mut self.held {
            *holder = holder.filter(|&i| strength(i).is_some());
        }
        let mut waiting: Vec<(usize, f32)> = (0..candidates.len())
            .filter(|i| !self.held.contains(&Some(*i)))
            .filter_map(|i| Some((i, strength(i)?)))
            .collect();
        waiting.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (light, power) in waiting {
            // The free map, else the weakest one that this is clearly stronger than.
            let weakest = (0..N).min_by(|&a, &b| {
                let of = |s: usize| self.held[s].and_then(strength).unwrap_or(f32::NEG_INFINITY);
                of(a).total_cmp(&of(b))
            });
            let Some(slot) = weakest else { break };
            let beaten = self.held[slot].and_then(strength);
            if beaten.is_none_or(|b| power > b * HYSTERESIS) {
                self.held[slot] = Some(light);
            }
        }
        self.held
    }
}

impl Pick<SHADOW_LIGHTS> {
    /// The maps `lights` have, seen from `eye`, and who holds each map.
    pub(crate) fn pick(
        &mut self,
        lights: &[DynLight],
        eye: Vec3,
    ) -> (Slots, [Option<(usize, Candidate)>; SHADOW_LIGHTS]) {
        let all: Vec<Option<Candidate>> =
            used(lights).iter().map(|l| Candidate::of(l, eye)).collect();
        let held = self.choose(&all);
        let mut slots = Slots::NONE;
        let holders = held.map(|h| h.and_then(|i| Some((i, all.get(i).copied().flatten()?))));
        for (slot, holder) in holders.iter().enumerate() {
            if let Some((light, _)) = holder {
                slots.0[*light] = slot as u32;
            }
        }
        (slots, holders)
    }
}

/// Faces of a cube map, in the order of its layers: +X, -X, +Y, -Y, +Z, -Z, each
/// with the axes of its image: right, up, forward. They are the orientation the
/// sampler gives a direction on that face, so a lookup finds what was drawn.
pub(crate) const FACES: [[Vec3; 3]; 6] = [
    [
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ],
    [
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
    ],
    [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
    ],
    [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, -1.0, 0.0),
    ],
    [
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
    ],
    [
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
    ],
];

/// The `Cube` of `shadow.wgsl` for face `face` of a light.
pub(crate) fn cube_bytes(pos: Vec3, radius: f32, face: usize) -> Vec<u8> {
    let [right, up, forward] = FACES[face];
    let mut bytes = Vec::with_capacity(64);
    for (v, w) in [(pos, radius), (right, 0.0), (up, 0.0), (forward, 0.0)] {
        for value in [v.x, v.y, v.z, w] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lit(brightness: f32) -> Option<Candidate> {
        Some(Candidate {
            pos: Vec3::ZERO,
            radius: 5.0,
            brightness,
        })
    }

    fn held(pick: &mut Pick<SHADOW_LIGHTS>, strengths: &[Option<f32>]) -> Vec<Option<usize>> {
        let all: Vec<_> = strengths.iter().map(|s| s.and_then(lit)).collect();
        pick.choose(&all).to_vec()
    }

    #[test]
    fn the_strongest_lights_get_the_maps() {
        let mut pick = Pick::<SHADOW_LIGHTS>::default();
        let got = held(&mut pick, &[Some(1.0), Some(5.0), Some(3.0), Some(0.5)]);
        let mut got: Vec<_> = got.into_iter().flatten().collect();
        got.sort();
        assert_eq!(got, [1, 2]);
    }

    #[test]
    fn two_lights_near_a_tie_do_not_swap_frame_to_frame() {
        let mut pick = Pick::<SHADOW_LIGHTS>::default();
        let first = held(&mut pick, &[Some(4.0), Some(3.9), Some(3.95)]);
        for flip in 0..20 {
            let (a, b, c) = if flip % 2 == 0 {
                (3.9, 4.0, 4.05)
            } else {
                (4.05, 3.9, 4.0)
            };
            let now = held(&mut pick, &[Some(a), Some(b), Some(c)]);
            assert_eq!(now, first, "frame {flip}");
        }
    }

    #[test]
    fn a_clearly_stronger_light_does_take_a_map() {
        let mut pick = Pick::<SHADOW_LIGHTS>::default();
        held(&mut pick, &[Some(4.0), Some(3.0), Some(1.0)]);
        let now = held(&mut pick, &[Some(4.0), Some(3.0), Some(9.0)]);
        assert!(now.contains(&Some(2)), "{now:?}");
    }

    #[test]
    fn a_light_that_goes_dark_gives_up_its_map() {
        let mut pick = Pick::<SHADOW_LIGHTS>::default();
        held(&mut pick, &[Some(4.0), Some(3.0)]);
        let now = held(&mut pick, &[None, Some(3.0)]);
        assert_eq!(now.iter().flatten().collect::<Vec<_>>(), [&1]);
    }

    #[test]
    fn a_light_that_is_not_a_place_casts_nothing() {
        let eye = Vec3::new(1.0, 2.0, 3.0);
        let at = |pos: Vec3, radius: f32, mean: f32| Candidate::new(pos, radius, mean, eye);
        let here = Vec3::new(4.0, 4.0, 4.0);
        assert!(at(here, 5.0, 1.0).is_some());
        assert!(at(Vec3::new(f32::NAN, 0.0, 0.0), 5.0, 1.0).is_none());
        assert!(at(Vec3::new(f32::INFINITY, 0.0, 0.0), 5.0, 1.0).is_none());
        assert!(at(here, 0.0, 1.0).is_none());
        assert!(at(here, -1.0, 1.0).is_none());
        assert!(at(here, f32::NAN, 1.0).is_none());
        assert!(at(here, 5.0, 0.0).is_none());
        assert!(at(here, 5.0, f32::NAN).is_none());
        assert!(Candidate::new(here, 5.0, 1.0, Vec3::new(f32::NAN, 0.0, 0.0)).is_none());
        assert!(Candidate::of(&DynLight::new(), eye).is_none());
    }

    #[test]
    fn the_faces_are_orthonormal_and_look_down_their_axes() {
        for [right, up, forward] in FACES {
            for (a, b) in [(right, up), (up, forward), (forward, right)] {
                assert!(a.dot(b).abs() < 1e-6);
            }
            assert!((right.length() * up.length() * forward.length() - 1.0).abs() < 1e-6);
        }
        let axes: Vec<_> = FACES.iter().map(|f| f[2]).collect();
        let expect = [
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
        ];
        assert_eq!(axes, expect);
    }
}
