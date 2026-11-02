//! Props (M9.8): things a script places that are not enemies. A switch to shoot,
//! a keycard to pick up, a terminal to use. A prop does nothing a script did not
//! ask for: it can be shot if it has health or an `onHit:`, picked up if it has
//! an `onPickup:`, and used if it has an `onUse:hold:`. Props are not solid.

use crate::combat::{Sphere, line_of_sight};
use crate::game::{Event, Game};
use crate::input::Input;
use crate::player::PLAYER_RADIUS;
use mscent_collide::{STEP_DT, Solids};
use mscent_math::Vec3;

/// How a prop looks. The renderer has a mesh for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropMesh {
    Switch,
    Keycard,
    Terminal,
    Crate,
}

impl PropMesh {
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name {
            "switch" => Some(Self::Switch),
            "keycard" => Some(Self::Keycard),
            "terminal" => Some(Self::Terminal),
            "crate" => Some(Self::Crate),
            _ => None,
        }
    }
}

/// What a kind of prop starts as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PropKind {
    pub mesh: PropMesh,
    pub radius: f32,
    pub hp: i32,
}

/// The kinds every script has.
pub(crate) fn built_in_kinds() -> Vec<(String, PropKind)> {
    let kind = |name: &str, mesh, radius, hp| (name.to_string(), PropKind { mesh, radius, hp });
    vec![
        kind("switch", PropMesh::Switch, 0.75, 25),
        kind("keycard", PropMesh::Keycard, 0.5, 0),
        kind("terminal", PropMesh::Terminal, 1.0, 0),
        kind("crate", PropMesh::Crate, 0.75, 50),
    ]
}

/// One placed prop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prop {
    pub pos: Vec3,
    pub radius: f32,
    pub mesh: PropMesh,
    pub hp: i32,
    /// False once destroyed or picked up.
    pub present: bool,
    /// A disabled prop cannot be picked up or used.
    pub enabled: bool,
    pub(crate) pickup: bool,
    /// Seconds the use key must be held, if it can be used.
    pub(crate) use_hold: Option<f32>,
    pub(crate) hit_reports: bool,
}

impl Prop {
    pub(crate) fn new(pos: Vec3, kind: &PropKind) -> Self {
        Self {
            pos,
            radius: kind.radius,
            mesh: kind.mesh,
            hp: kind.hp,
            present: true,
            enabled: true,
            pickup: false,
            use_hold: None,
            hit_reports: false,
        }
    }

    /// Whether a shot stops here.
    pub(crate) fn shootable(&self) -> bool {
        self.present && (self.hp > 0 || self.hit_reports)
    }

    pub(crate) fn sphere(&self) -> Sphere {
        Sphere {
            centre: self.pos,
            radius: self.radius,
        }
    }

    /// Applies a shot's damage; returns true if it destroyed the prop. A prop
    /// with no health takes the hit and stays.
    pub(crate) fn damage(&mut self, amount: i32) -> bool {
        if self.hp <= 0 {
            return false;
        }
        self.hp = self.hp.saturating_sub(amount.max(0));
        if self.hp > 0 {
            return false;
        }
        self.present = false;
        true
    }
}

/// How far beyond a usable prop's edge the player can reach it.
pub(crate) const USE_REACH: f32 = 2.0;
/// Cosine of the widest angle between where the player looks and the prop.
const USE_AIM_COS: f32 = 0.7;

/// The player holding the use key on something.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub(crate) struct UseState {
    pub prop: Option<usize>,
    pub held: f32,
    /// It has fired for this press, so holding on does not fire it again.
    pub done: bool,
}

impl Game {
    /// Picks up what the player touches and counts the use key. Only a level
    /// with a script has props, so a level without one never gets here.
    pub(crate) fn update_props(&mut self, input: &Input) {
        if self.props.is_empty() {
            return;
        }
        let p = self.player.pos();
        for (i, prop) in self.props.iter_mut().enumerate() {
            let reach = prop.radius + PLAYER_RADIUS;
            let touching = (prop.pos - p).length_squared() <= reach * reach;
            if prop.present && prop.enabled && prop.pickup && touching {
                prop.present = false;
                self.events.push(Event::PropPicked(i));
            }
        }
        self.update_use(input);
    }

    /// The nearest usable prop the player can reach, is looking at and can see.
    fn usable_in_reach(&self) -> Option<usize> {
        let (p, forward) = (self.player.pos(), self.player.forward());
        let solids = Solids::new(&self.arena.bsp, &self.solids);
        let mut best: Option<(usize, f32)> = None;
        for (i, prop) in self.props.iter().enumerate() {
            if !(prop.present && prop.enabled && prop.use_hold.is_some()) {
                continue;
            }
            let to = prop.pos - p;
            let dist = to.length();
            let aimed = to
                .normalized()
                .is_some_and(|d| d.dot(forward) >= USE_AIM_COS);
            let near = dist <= prop.radius + USE_REACH;
            if near
                && aimed
                && best.is_none_or(|(_, b)| dist < b)
                && line_of_sight(solids, p, prop.pos)
            {
                best = Some((i, dist));
            }
        }
        best.map(|(i, _)| i)
    }

    fn update_use(&mut self, input: &Input) {
        let target = self.usable_in_reach();
        self.use_target = target;
        if !input.use_key {
            self.use_state = UseState::default();
            return;
        }
        if self.use_state.prop != target {
            self.use_state = UseState {
                prop: target,
                held: 0.0,
                done: false,
            };
        }
        let Some(index) = target else { return };
        let hold = self
            .props
            .get(index)
            .and_then(|p| p.use_hold)
            .unwrap_or(0.0);
        self.use_state.held += STEP_DT;
        if !self.use_state.done && self.use_state.held >= hold {
            self.use_state.done = true;
            self.events.push(Event::PropUsed(index));
        }
    }

    /// Where the use key stands, for the HUD: whether something is in reach,
    /// and how far along holding it is, `0..=1`.
    pub fn use_prompt(&self) -> Option<f32> {
        let index = self.use_target?;
        let hold = self.props.get(index)?.use_hold?;
        if self.use_state.prop != Some(index) || hold <= 0.0 {
            return Some(0.0);
        }
        Some((self.use_state.held / hold).min(1.0))
    }
}
