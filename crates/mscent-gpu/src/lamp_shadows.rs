//! Shadows from the level's static lamps, cast by what moves.
//!
//! A lamp's light is in the lightmap already, baked with the level standing
//! still, so nothing that moves has ever shadowed it. The lamps nearest to
//! something that moves get a cube map like a dynamic light's (`shadows.rs`), and
//! a face darkens its baked light by the share of the lamp that the map says is
//! blocked (`lamp_shadowed` in `face.wgsl`). The maps hold movers only: a lamp
//! the level's walls already shade is dark in the lightmap, and there is nothing
//! left to take from it.

use mscent_math::Vec3;
use mscent_world::lightmap::{Cone, PointLight};

use crate::shadow_lights::{Candidate, Pick};

/// Static lamps that cast in one frame, each with a map after the dynamic lights'.
pub(crate) const LAMP_SHADOWS: usize = 2;
// `Frame` in common.wgsl holds this many: a uniform's arrays cannot take their
// length from an override.
const _: () = assert!(LAMP_SHADOWS == 2);
/// The power of `1 - distance / radius` the baker lights with (`soft.rs` in
/// mscent-world), which the faces' shader takes the lamp's share with.
pub(crate) const FALLOFF_POWER: f64 = 3.0;
/// Bytes of `lamps`, `lamp_colors`, `lamp_bounds`, `lamp_aims` and `lamp_edges`
/// in `Frame`.
pub(crate) const BYTES: u64 = 80 * LAMP_SHADOWS as u64;

/// What `Frame` holds of a lamp that casts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct LampFrame {
    pub(crate) pos: Vec3,
    pub(crate) radius: f32,
    pub(crate) color: Vec3,
    /// A sphere that holds everything that casts into the lamp's map, so that a
    /// face the lamp's light reaches past it can skip the lookup.
    pub(crate) bound: (Vec3, f32),
    /// Where a projector shines; a lamp with none lights all round.
    pub(crate) cone: Option<Cone>,
}

/// The lamps that cast, by map.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Lamps(pub(crate) [Option<LampFrame>; LAMP_SHADOWS]);

impl Lamps {
    /// Appends the `Frame` bytes. A lamp with no map gets a radius of 0, which
    /// the shader skips.
    pub(crate) fn push(self, bytes: &mut Vec<u8>) {
        let each = |bytes: &mut Vec<u8>, at: fn(&LampFrame) -> [f32; 4]| {
            for lamp in self.0 {
                for value in lamp.as_ref().map_or([0.0; 4], at) {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        };
        each(bytes, |l| [l.pos.x, l.pos.y, l.pos.z, l.radius]);
        each(bytes, |l| [l.color.x, l.color.y, l.color.z, 0.0]);
        each(bytes, |l| {
            [l.bound.0.x, l.bound.0.y, l.bound.0.z, l.bound.1]
        });
        // The cone, as `Cone::share` takes it: the axis and the outer cosine, then
        // the inner cosine, whether there is a cone at all and whether its edge is hard.
        each(bytes, |l| {
            l.cone.map_or([0.0; 4], |c| {
                let d = c.dir();
                [d.x, d.y, d.z, c.edge_cosines().1]
            })
        });
        each(bytes, |l| {
            l.cone.map_or([0.0; 4], |c| {
                [
                    c.edge_cosines().0,
                    1.0,
                    f32::from(u8::from(c.is_hard())),
                    0.0,
                ]
            })
        });
    }
}

/// Which lamp holds each map, kept from frame to frame.
#[derive(Default)]
pub(crate) struct LampPick {
    pick: Pick<LAMP_SHADOWS>,
}

/// A lamp's colour as one brightness, as the dynamic lights' is.
fn mean(light: &PointLight) -> f32 {
    (light.color.x + light.color.y + light.color.z) / 3.0
}

impl LampPick {
    /// The lamps that hold maps, seen from `eye`, by map. Only a lamp that
    /// `reaches` something that moves can cast, so only those compete.
    pub(crate) fn pick(
        &mut self,
        lamps: &[PointLight],
        eye: Vec3,
        reaches: impl Fn(Vec3, f32) -> bool,
    ) -> [Option<(usize, Candidate)>; LAMP_SHADOWS] {
        let all: Vec<Option<Candidate>> = lamps
            .iter()
            .map(|l| {
                Candidate::new(l.position, l.radius, mean(l), eye)
                    .filter(|c| l.color.is_finite() && reaches(c.pos, c.radius))
            })
            .collect();
        let held = self.pick.choose(&all);
        held.map(|h| h.and_then(|i| Some((i, all.get(i).copied().flatten()?))))
    }
}

/// The `Frame` data of the lamps that hold maps. `bound_of` is the sphere that
/// holds what casts into a lamp's sphere; a lamp it says nothing of is taken to
/// be cast into from anywhere in its reach.
pub(crate) fn frame_lamps(
    lamps: &[PointLight],
    held: &[Option<(usize, Candidate)>; LAMP_SHADOWS],
    bound_of: impl Fn(Vec3, f32) -> Option<(Vec3, f32)>,
) -> Lamps {
    Lamps(held.map(|h| {
        let (i, c) = h?;
        Some(LampFrame {
            pos: c.pos,
            radius: c.radius,
            color: lamps.get(i)?.color,
            bound: bound_of(c.pos, c.radius).unwrap_or((c.pos, c.radius)),
            cone: lamps.get(i)?.cone,
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lamp(x: f32, radius: f32, color: f32) -> PointLight {
        PointLight {
            position: Vec3::new(x, 0.0, 0.0),
            color: Vec3::new(color, color, color),
            radius,
            cone: None,
        }
    }

    fn held(pick: &mut LampPick, lamps: &[PointLight], reach: bool) -> Vec<usize> {
        let got = pick.pick(lamps, Vec3::ZERO, |_, _| reach);
        let mut got: Vec<_> = got.iter().flatten().map(|h| h.0).collect();
        got.sort();
        got
    }

    #[test]
    fn the_two_brightest_lamps_that_reach_something_hold_maps() {
        let lamps = [
            lamp(2.0, 9.0, 1.0),
            lamp(2.0, 9.0, 4.0),
            lamp(2.0, 9.0, 2.0),
        ];
        let mut pick = LampPick::default();
        assert_eq!(held(&mut pick, &lamps, true), [1, 2]);
        assert!(held(&mut LampPick::default(), &lamps, false).is_empty());
    }

    #[test]
    fn only_a_lamp_that_reaches_a_caster_competes() {
        let lamps = [lamp(2.0, 9.0, 9.0), lamp(30.0, 9.0, 1.0)];
        let mut pick = LampPick::default();
        let got = pick.pick(&lamps, Vec3::ZERO, |pos, _| pos.x > 10.0);
        assert_eq!(got.iter().flatten().map(|h| h.0).collect::<Vec<_>>(), [1]);
    }

    #[test]
    fn a_lamp_keeps_its_map_against_a_near_tie() {
        let mut lamps = [
            lamp(2.0, 9.0, 4.0),
            lamp(2.0, 9.0, 3.9),
            lamp(2.0, 9.0, 3.95),
        ];
        let mut pick = LampPick::default();
        let first = held(&mut pick, &lamps, true);
        for flip in 0..20 {
            let shade = if flip % 2 == 0 {
                [3.9, 4.0, 4.05]
            } else {
                [4.05, 3.9, 4.0]
            };
            for (l, c) in lamps.iter_mut().zip(shade) {
                l.color = Vec3::new(c, c, c);
            }
            assert_eq!(held(&mut pick, &lamps, true), first, "frame {flip}");
        }
    }

    #[test]
    fn a_lamp_that_is_not_a_place_casts_nothing() {
        let mut bad = [lamp(2.0, 9.0, 1.0); 4];
        bad[0].position = Vec3::new(f32::NAN, 0.0, 0.0);
        bad[1].radius = 0.0;
        bad[2].radius = f32::INFINITY;
        bad[3].color = Vec3::new(f32::NAN, 1.0, 1.0);
        assert!(held(&mut LampPick::default(), &bad, true).is_empty());
    }

    #[test]
    fn a_frame_names_each_lamp_that_holds_a_map_and_no_other() {
        let lamps = [lamp(2.0, 9.0, 1.0)];
        let mut pick = LampPick::default();
        let got = pick.pick(&lamps, Vec3::ZERO, |_, _| true);
        let bound = (Vec3::new(1.0, 2.0, 3.0), 0.5);
        let frame = frame_lamps(&lamps, &got, |_, _| Some(bound));
        let held = frame.0[0].expect("the lamp holds a map");
        assert_eq!(held.pos, lamps[0].position);
        assert_eq!(
            (held.radius, held.color, held.bound),
            (9.0, lamps[0].color, bound)
        );
        assert_eq!(frame.0[1], None);
        let mut bytes = Vec::new();
        frame.push(&mut bytes);
        assert_eq!(bytes.len() as u64, BYTES);
        assert_eq!(&bytes[16..32], &[0; 16], "the second lamp reaches nothing");
        // The first lamp's bound is the third vec4 of three: after two lamps'
        // positions and two lamps' colours.
        let at = 4 * 16;
        assert_eq!(&bytes[at + 12..at + 16], &0.5f32.to_le_bytes());
        // With no word of what casts, the bound is the lamp's whole reach.
        let open = frame_lamps(&lamps, &got, |_, _| None);
        assert_eq!(open.0[0].map(|l| l.bound), Some((lamps[0].position, 9.0)));
    }
}
