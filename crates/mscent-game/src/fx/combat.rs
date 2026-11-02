//! Combat effects: what the session hands shots and kills to. Gameplay stays
//! hitscan, so a bolt is cosmetic: damage landed on the firing step, and the
//! bolt only decides when the impact effect appears.

use mscent_math::Vec3;
use mscent_math::epsilon::SURFACE_PROBE;
use mscent_render::particles::{Mask, ParticleDraws, SpriteDraw, streak_from_world};
use mscent_render::scene::Scene;
use mscent_render::shade::mul_rgb;
use mscent_render::{Camera, DynLight, Fog, Framebuffer, Quality};
use mscent_world::Bsp;

use super::budget::{self, Keep, Limits, Sink, Tag, Tags, Tally};
use super::class::Blend;
use super::class::MAX_TOTAL_CAPACITY;
use super::collect::{self, FogView};
use super::decal::{Decal, Decals, FaceOutline, MAX_DECALS};
use super::lights::{LightSpec, Lights};
use super::recipes;
use super::{Effect, EffectArgs, Fx};

mod big;
pub use big::ViewKick;

/// How fast a bolt's head travels, in units/s.
const BOLT_SPEED: f32 = 400.0;
/// The longest a bolt takes to arrive, so any range is covered in a handful of
/// frames and a kill's explosion stays close to the shot that caused it.
const MAX_FLIGHT: f32 = 0.08;
const BOLT_TAIL: f32 = 4.0;
const MAX_BOLTS: usize = 32;
/// Every particle, plus the two streaks and a glow of every bolt.
const MAX_DRAWS: usize = MAX_TOTAL_CAPACITY + 3 * MAX_BOLTS;
/// Past the front of a drone, which measures 1.0 from its centre on real depth,
/// so its flash is not drawn behind its own body.
const ENEMY_MUZZLE_REACH: f32 = 1.3;
const GLOW_RADIUS: f32 = 0.22;
const PLAYER_EDGE: u32 = 0xff20_90b0;
const ENEMY_EDGE: u32 = 0xffc0_5010;
const CORE: u32 = 0xffff_ffff;
const NS_PER_SEC: f32 = 1.0e9;
/// Fixed so effects replay with a scripted run.
const SEED: u64 = 0xF1A5_4000;

/// What a shot does where it ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impact {
    /// Out of range or at the viewer: only the bolt shows.
    None,
    /// A wall, with its material id and `0xffRRGGBB` mean colour.
    Surface {
        material: u16,
        tint: u32,
    },
    Drone,
}

/// A shot as effects see it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShotFx {
    pub by_player: bool,
    pub muzzle: Vec3,
    pub end: Vec3,
    pub normal: Vec3,
    pub impact: Impact,
}

#[derive(Clone, Copy)]
struct Bolt {
    origin: Vec3,
    dir: Vec3,
    dist: f32,
    by_player: bool,
    age_ns: u64,
    flight_ns: u64,
    arrival: Option<(Effect, EffectArgs)>,
    /// The scorch, which appears with the impact.
    mark: Option<Decal>,
    glow: Option<LightSpec>,
}

impl Bolt {
    /// Head and tail along the shot. The head starts one tail length out, so a
    /// shot shorter than a frame is still seen for that frame.
    fn span(&self) -> (f32, f32) {
        let f = self.age_ns as f32 / self.flight_ns as f32;
        let head = (self.dist * f).max(BOLT_TAIL).min(self.dist);
        (head, (head - BOLT_TAIL).max(0.0))
    }
}

/// Particles and bolts of one play session.
pub struct CombatFx {
    fx: Fx,
    lights: Lights,
    blast_view: big::BlastView,
    bolts: [Option<Bolt>; MAX_BOLTS],
    decals: Decals,
    limits: Limits,
    keep: Keep,
    tags: Tags,
}

impl Default for CombatFx {
    fn default() -> Self {
        Self::new()
    }
}

impl CombatFx {
    pub fn new() -> Self {
        Self {
            fx: Fx::new(&recipes::CLASSES, SEED),
            lights: Lights::new(),
            blast_view: big::BlastView::default(),
            bolts: [None; MAX_BOLTS],
            decals: Decals::new(),
            limits: Limits::of(Quality::High),
            keep: Keep::default(),
            tags: Tags::with_capacity(MAX_DRAWS),
        }
    }

    /// What this quality may spend from now on. Particles already alive stay.
    pub fn set_quality(&mut self, quality: Quality) {
        self.limits = Limits::of(quality);
        self.fx.set_density(self.limits.density);
        self.lights.set_limit(self.limits.lights);
    }

    pub fn clear(&mut self) {
        self.fx.clear();
        self.lights.clear();
        self.blast_view.clear();
        self.bolts = [None; MAX_BOLTS];
        self.decals.clear();
        self.keep = Keep::default();
    }

    /// Live particles, for tests and stats.
    pub fn live(&self) -> usize {
        self.fx.live()
    }

    /// Scorch marks on walls.
    pub fn decals(&self) -> usize {
        self.decals.len()
    }

    /// Bolts in flight.
    pub fn bolts(&self) -> usize {
        self.bolts.iter().flatten().count()
    }

    /// A light of its own, for effects that are not a recipe here.
    pub fn light(&mut self, spec: LightSpec) {
        self.lights.add(spec, None);
    }

    /// The lights for this frame, to hand to the renderer. A new light is
    /// placed in `scene` first, which floods the portals once for it.
    pub fn lights(&mut self, scene: &Scene) -> &[DynLight] {
        self.lights.active(scene)
    }

    /// Lights alive, for tests and stats.
    pub fn live_lights(&self) -> usize {
        self.lights.live()
    }

    /// Runs the particle clock and the bolts forward by `elapsed_ns`.
    pub fn advance(&mut self, elapsed_ns: u64, bsp: &Bsp) {
        self.blast_view.advance(elapsed_ns);
        for slot in &mut self.bolts {
            let Some(bolt) = slot else { continue };
            bolt.age_ns = bolt.age_ns.saturating_add(elapsed_ns);
            if bolt.age_ns >= bolt.flight_ns {
                if let Some((effect, args)) = bolt.arrival {
                    self.fx.spawn(effect, &args);
                }
                if let Some(mark) = bolt.mark {
                    self.decals.push(mark);
                }
                if let Some(glow) = bolt.glow {
                    self.lights.add(glow, Some(bsp));
                }
                *slot = None;
            }
        }
        self.fx.advance(elapsed_ns, bsp);
        self.lights.advance(elapsed_ns);
    }

    /// A muzzle flash now, a bolt, and the impact when the bolt arrives. With
    /// every bolt slot taken the impact happens at once, without a bolt.
    pub fn shot(&mut self, s: &ShotFx) {
        self.shot_on(s, None);
    }

    /// [`Self::shot`] for a wall hit whose face is known, so the scorch is cut
    /// to the face.
    pub fn shot_on(&mut self, s: &ShotFx, face: Option<&FaceOutline>) {
        let muzzle: Effect = if s.by_player {
            recipes::MUZZLE_PLAYER
        } else {
            recipes::MUZZLE_ENEMY
        };
        self.fx.spawn(muzzle, &args_at(s.muzzle, Vec3::ZERO));
        let arrival = arrival_of(s);
        let (Some(dir), true) = ((s.end - s.muzzle).normalized(), s.end.is_finite()) else {
            return;
        };
        let dist = (s.end - s.muzzle).length();
        let flight = (dist / BOLT_SPEED).min(MAX_FLIGHT);
        let bolt = Bolt {
            origin: s.muzzle,
            dir,
            dist,
            by_player: s.by_player,
            age_ns: 0,
            flight_ns: ((flight * NS_PER_SEC) as u64).max(1),
            arrival,
            mark: mark_of(s, face),
            glow: glow_of(s),
        };
        match self.bolts.iter_mut().find(|b| b.is_none()) {
            Some(slot) => *slot = Some(bolt),
            None => {
                if let Some((effect, args)) = arrival {
                    self.fx.spawn(effect, &args);
                }
                if let Some(mark) = bolt.mark {
                    self.decals.push(mark);
                }
                if let Some(glow) = bolt.glow {
                    self.lights.add(glow, None);
                }
            }
        }
    }

    /// A drone blown apart, in its own colour `tint`.
    pub fn drone_explosion(&mut self, pos: Vec3, tint: u32) {
        let args = EffectArgs {
            surface: tint & 0x00ff_ffff,
            ..args_at(pos, Vec3::ZERO)
        };
        self.fx.spawn(recipes::DRONE_EXPLOSION, &args);
        let glow = recipes::EXPLOSION_LIGHT.at(pos, Vec3::ZERO);
        self.lights.add(glow, None);
    }

    /// Draw lists that already hold the fullest possible frame, so play never
    /// grows them.
    pub fn new_draws() -> ParticleDraws {
        ParticleDraws::with_capacity(MAX_DRAWS, MAX_DECALS)
    }

    /// Replaces `out` with this frame's draws, fitted to the budget. `depth` is
    /// the last frame, if there is one, for judging what the scene hides. No
    /// allocation once the lists have grown to a frame's size.
    pub fn collect(
        &mut self,
        cam: &Camera,
        fog: Option<Fog>,
        depth: Option<&Framebuffer>,
        out: &mut ParticleDraws,
    ) {
        out.clear();
        self.tags.clear();
        let fog = FogView(fog);
        let (w, h) = (cam.width() as f32, cam.height() as f32);
        let mut sink = Sink {
            out,
            tags: &mut self.tags,
            width: w,
            height: h,
        };
        collect::particles(&self.fx, &recipes::CLASSES, cam, &fog, &mut sink);
        for bolt in self.bolts.iter().flatten() {
            draw_bolt(bolt, cam, &fog, &mut sink);
        }
        self.decals.collect(cam, &fog, &mut sink);
        let tally = Tally::of(out, &self.tags, depth, w, h);
        self.keep.follow(tally.fit(self.limits.screens * w * h));
        budget::thin(out, &self.tags, self.keep);
        collect::sort_back_to_front(&mut out.over);
    }
}

/// Where an enemy at `origin` firing toward `end` shows its muzzle.
pub fn enemy_muzzle(origin: Vec3, end: Vec3) -> Vec3 {
    let to = end - origin;
    let reach = ENEMY_MUZZLE_REACH.min(to.length());
    origin + to.normalized().unwrap_or(Vec3::ZERO) * reach
}

fn args_at(pos: Vec3, dir: Vec3) -> EffectArgs {
    EffectArgs {
        pos,
        dir,
        end: pos,
        scale: 1.0,
        surface: 0,
        volume: None,
    }
}

/// The scorch a wall hit leaves; nothing for any other impact.
fn mark_of(s: &ShotFx, face: Option<&FaceOutline>) -> Option<Decal> {
    let Impact::Surface { material, .. } = s.impact else {
        return None;
    };
    Decal::new(s.end, s.normal, recipes::surface_mark(material), face)
}

fn arrival_of(s: &ShotFx) -> Option<(Effect, EffectArgs)> {
    // Off the surface, so nothing is born inside the wall it just struck.
    let at = s.end + s.normal * SURFACE_PROBE;
    match s.impact {
        Impact::None => None,
        Impact::Surface { material, tint } => Some((
            recipes::surface_hit(material),
            EffectArgs {
                surface: tint & 0x00ff_ffff,
                ..args_at(at, s.normal)
            },
        )),
        Impact::Drone => {
            let shot = (s.end - s.muzzle).normalized().unwrap_or(Vec3::ZERO);
            let reflected = shot - s.normal * (2.0 * shot.dot(s.normal));
            Some((
                recipes::HIT_DRONE,
                EffectArgs {
                    surface: DRONE_TINT,
                    ..args_at(at, reflected)
                },
            ))
        }
    }
}

/// The drone's red hull, for its chips.
pub const DRONE_TINT: u32 = 0x00c0_2828;

/// The flash of a bolt's arrival, if it lands on something that shows it.
fn glow_of(s: &ShotFx) -> Option<LightSpec> {
    let at = s.end + s.normal * SURFACE_PROBE;
    match s.impact {
        Impact::None => None,
        Impact::Surface { .. } => Some(recipes::SURFACE_LIGHT.at(at, s.normal)),
        Impact::Drone => Some(recipes::DRONE_HIT_LIGHT.at(at, s.normal)),
    }
}

fn draw_bolt(b: &Bolt, cam: &Camera, fog: &FogView, out: &mut Sink) {
    let (head, tail) = b.span();
    let (a, z) = (b.origin + b.dir * tail, b.origin + b.dir * head);
    let edge = if b.by_player { PLAYER_EDGE } else { ENEMY_EDGE };
    let depth = -cam.world_to_camera(z).z;
    let Some(fade) = fog.fade(depth) else { return };
    // Thicker close up, with a white core inside a coloured edge.
    let width = (6.0 / depth.max(1.0)).clamp(1.0, 3.0) as u8;
    let tinted = |c: u32| mul_rgb(c, fade);
    if let Some(s) = streak_from_world(cam, a, z, width + 2, tinted(edge)) {
        out.streak(s, Tag::ESSENTIAL);
    }
    if let Some(s) = streak_from_world(cam, a, z, width, tinted(CORE)) {
        out.streak(s, Tag::ESSENTIAL);
    }
    let Some(p) = cam.project(cam.world_to_camera(z)) else {
        return;
    };
    let glow = SpriteDraw {
        x: p.x,
        y: p.y,
        radius: GLOW_RADIUS * cam.focal() * p.inv_z,
        inv_z: p.inv_z,
        tint: tinted(edge),
        mask: Mask::Soft,
    };
    out.sprite(glow, Blend::Add, Tag::ESSENTIAL);
}

#[cfg(test)]
#[path = "../fx_combat_tests.rs"]
mod fx_combat_tests;
