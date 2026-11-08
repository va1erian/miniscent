//! Enemy kinds kept in a level (M19.1, design in `docs/enemy-designer.md`).
//!
//! A [`Kind`] is a name, the numbers the simulation plays by, and a
//! [`Blueprint`] the renderers draw. `Entity::Enemy::kind` indexes the level's
//! table. A level with no kinds of its own plays the stock table, [`stock_kinds`]:
//! `drone` and `hunter`, with the stats every level had before kinds were kept in
//! it, so old levels and their replays play as they did.
//!
//! Kinds are untrusted like the rest of a level. [`Level::check_kinds`] is what
//! the reader enforces, and it judges the whole table's build cost from the
//! blueprints alone, before anything is built.

use std::borrow::Cow;

use crate::entity::{Entity, Level};
use crate::model::{self, Blueprint, Class, DEFAULT_SCALE, MAX_MODEL_NS, estimated_ns};

/// Most kinds a level may hold. Each is built when the level loads.
pub const MAX_LEVEL_KINDS: usize = 32;
/// Most estimated build time for all of a level's kinds together, in
/// nanoseconds: a few costly kinds or many cheap ones.
pub const MAX_LEVEL_MODEL_NS: u64 = 4 * MAX_MODEL_NS;
/// Kinds in the stock table.
pub const STOCK_KINDS: usize = 2;
/// Longest kind name, in bytes.
pub const MAX_KIND_NAME: usize = 32;
/// Largest hit points, damage, speed, sight or attack range.
pub const MAX_KIND_STAT: f32 = 1.0e6;
/// Largest body radius, in world units. A body wider than this fits no room.
pub const MAX_KIND_RADIUS: f32 = 64.0;
/// Longest cooldown or patience, in seconds.
pub const MAX_KIND_SECONDS: f32 = 3600.0;

/// What a kind plays by. Times are in seconds, so a kind does not depend on the
/// game's step rate; the game turns them into whole steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KindStats {
    pub hp: u32,
    /// Radius of the body that collides with the world, in world units.
    pub radius: f32,
    /// Chase speed, in world units a second.
    pub speed: f32,
    pub sight: f32,
    pub attack_range: f32,
    pub damage: u32,
    /// Seconds between shots.
    pub cooldown: f32,
    /// Seconds a chaser hunts where it last saw its target before giving up.
    pub lose: f32,
}

impl KindStats {
    /// The stock drone's numbers, which every enemy had before kinds.
    pub const DRONE: KindStats = KindStats {
        hp: 50,
        radius: 0.75,
        speed: 5.0,
        sight: 40.0,
        attack_range: 9.0,
        damage: 6,
        cooldown: 0.5,
        lose: 3.0,
    };

    /// The stock hunter's: tougher, faster and harder hitting than a drone.
    pub const HUNTER: KindStats = KindStats {
        hp: 100,
        speed: 7.0,
        damage: 12,
        cooldown: 1.0,
        ..KindStats::DRONE
    };

    /// Whether every number is finite and in range.
    pub fn is_valid(&self) -> bool {
        let max = MAX_KIND_STAT as u32;
        let within = |v: f32, hi: f32| v.is_finite() && (0.0..=hi).contains(&v);
        self.hp <= max
            && self.damage <= max
            && within(self.radius, MAX_KIND_RADIUS)
            && self.radius > 0.0
            && [self.speed, self.sight, self.attack_range]
                .iter()
                .all(|&v| within(v, MAX_KIND_STAT))
            && within(self.cooldown, MAX_KIND_SECONDS)
            && within(self.lose, MAX_KIND_SECONDS)
    }
}

/// One enemy design in a level.
#[derive(Clone, Debug, PartialEq)]
pub struct Kind {
    /// What scripts and the editor call it: `[a-z0-9_-]`, unique in the level.
    pub name: String,
    pub stats: KindStats,
    pub blueprint: Blueprint,
}

/// Why a level's kinds were refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KindError {
    /// More than [`MAX_LEVEL_KINDS`].
    TooMany,
    /// The kind at this index has an empty, long or odd name.
    BadName(usize),
    /// The kind at this index has the name of one before it.
    DuplicateName(usize),
    /// The kind at this index has a stat out of range.
    BadStats(usize),
    /// The kind at this index has a blueprint the reader refuses.
    BadBlueprint(usize),
    /// Building every kind would take more than [`MAX_LEVEL_MODEL_NS`].
    TooCostly,
    /// The enemy entity at this index names a kind past the table.
    NoSuchKind(usize),
}

/// Whether `name` may name a kind.
pub fn valid_kind_name(name: &str) -> bool {
    let plain = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-';
    !name.is_empty() && name.len() <= MAX_KIND_NAME && name.bytes().all(plain)
}

/// The table a level without kinds of its own plays: `0` is the drone, which
/// wears the sentry, and `1` the hunter, which wears the dart.
pub fn stock_kinds() -> Vec<Kind> {
    vec![
        stock(
            "drone",
            KindStats::DRONE,
            include_str!("../../../assets/enemies/sentry.msb"),
        ),
        stock(
            "hunter",
            KindStats::HUNTER,
            include_str!("../../../assets/enemies/dart.msb"),
        ),
    ]
}

fn stock(name: &str, stats: KindStats, text: &str) -> Kind {
    // The stock files are part of the build and `stock_kinds_read` reads them,
    // so the empty design is never reached; it keeps a broken asset from
    // being a panic.
    let blueprint = model::read(text.as_bytes()).unwrap_or_else(|_| Blueprint {
        name: name.to_string(),
        class: Class::Drone,
        plan: None,
        scale: DEFAULT_SCALE,
        skins: Vec::new(),
        parts: Vec::new(),
        hits: Vec::new(),
    });
    Kind {
        name: name.to_string(),
        stats,
        blueprint,
    }
}

impl Level {
    /// The kinds this level plays: its own, or the stock table if it has none.
    pub fn kind_table(&self) -> Cow<'_, [Kind]> {
        if self.kinds.is_empty() {
            Cow::Owned(stock_kinds())
        } else {
            Cow::Borrowed(&self.kinds)
        }
    }

    /// How many kinds an enemy may choose from.
    pub fn kind_count(&self) -> usize {
        if self.kinds.is_empty() {
            STOCK_KINDS
        } else {
            self.kinds.len()
        }
    }

    /// Checks the kinds and every enemy's kind against them. Blueprints are
    /// judged by the reader, which a file's kinds have been through; a kind made
    /// in code is held to it when the level is written and read back.
    pub fn check_kinds(&self) -> Result<(), KindError> {
        if self.kinds.len() > MAX_LEVEL_KINDS {
            return Err(KindError::TooMany);
        }
        let mut cost: u64 = 0;
        for (i, k) in self.kinds.iter().enumerate() {
            if !valid_kind_name(&k.name) {
                return Err(KindError::BadName(i));
            }
            if self.kinds[..i].iter().any(|o| o.name == k.name) {
                return Err(KindError::DuplicateName(i));
            }
            if !k.stats.is_valid() {
                return Err(KindError::BadStats(i));
            }
            cost = cost.saturating_add(estimated_ns(&k.blueprint));
        }
        if cost > MAX_LEVEL_MODEL_NS {
            return Err(KindError::TooCostly);
        }
        self.check_enemy_kinds()
    }

    fn check_enemy_kinds(&self) -> Result<(), KindError> {
        let count = self.kind_count();
        let past =
            |e: &Entity| matches!(e, Entity::Enemy { kind, .. } if usize::from(*kind) >= count);
        match self.entities.iter().position(past) {
            Some(i) => Err(KindError::NoSuchKind(i)),
            None => Ok(()),
        }
    }
}
