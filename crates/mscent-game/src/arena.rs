//! World instantiation (M6.2): turns a [`Level`] into the immutable data a
//! game runs on. Dynamic state (player, enemies) is created from it by
//! [`crate::Game`], so a restart never has to reload the level.

use mscent_collide::MoverShape;
use mscent_math::Vec3;
use mscent_world::{Bsp, CsgError, Entity, LabelError, Level, compile};
use std::collections::BTreeMap;
use std::sync::Arc;

/// A placed point light, handed to the renderer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightSpec {
    pub pos: Vec3,
    pub color: Vec3,
    pub radius: f32,
    pub intensity: f32,
}

/// A win zone.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Goal {
    pub pos: Vec3,
    pub radius: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnemySpawn {
    pub pos: Vec3,
    pub kind: u8,
}

/// A named point for scripts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MarkerSpec {
    pub pos: Vec3,
    pub yaw: f32,
}

/// A box scripts are told the player entered or left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneSpec {
    pub pos: Vec3,
    pub half: Vec3,
}

/// What a level name refers to: an index into the arena's list of that kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Thing {
    Mover(usize),
    Zone(usize),
    Marker(usize),
    Enemy(usize),
}

/// The level's names and tags, for scripts. Ordered, so anything listing them
/// does so the same way every run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LevelNames {
    pub by_name: BTreeMap<String, Thing>,
    pub tagged: BTreeMap<String, Vec<Thing>>,
}

impl LevelNames {
    /// Markers with this tag, then those whose names start with it.
    pub(crate) fn markers_for(&self, prefix: &str) -> Vec<usize> {
        let tagged = self.tagged.get(prefix).into_iter().flatten().copied();
        let named = self
            .by_name
            .range(prefix.to_string()..)
            .take_while(|(name, _)| name.starts_with(prefix))
            .map(|(_, thing)| *thing);
        let mut found: Vec<usize> = Vec::new();
        for thing in tagged.chain(named) {
            if let Thing::Marker(i) = thing
                && !found.contains(&i)
            {
                found.push(i);
            }
        }
        found
    }

    /// For each function taking a level name, the names it can take. A literal
    /// outside these is a load error, even in a handler that never runs.
    pub(crate) fn selector_names(&self) -> Vec<(&'static str, Vec<&str>)> {
        let of = |wanted: fn(&Thing) -> bool| -> Vec<&str> {
            let all = self.by_name.iter();
            all.filter(|(_, t)| wanted(t))
                .map(|(n, _)| n.as_str())
                .collect()
        };
        vec![
            ("mover:", of(|t| matches!(t, Thing::Mover(_)))),
            ("zone:", of(|t| matches!(t, Thing::Zone(_)))),
            ("marker:", of(|t| matches!(t, Thing::Marker(_)))),
            ("enemy:", of(|t| matches!(t, Thing::Enemy(_)))),
        ]
    }
}

/// A named group of brushes that moves, as authored.
#[derive(Clone, Debug, PartialEq)]
pub struct MoverSpec {
    pub name: String,
    pub shape: Arc<MoverShape>,
}

/// Why a level cannot be played.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadError {
    Csg(CsgError),
    NoPlayerStart,
    NoGoal,
    /// Entity at this index in the level fails validation.
    InvalidEntity(usize),
    /// Player start or enemy at this entity index is inside solid rock.
    SpawnInSolid(usize),
    /// Names, tags or mover groups are inconsistent.
    Labels(LabelError),
}

/// Immutable per-level game data.
#[derive(Clone, Debug, PartialEq)]
pub struct Arena {
    pub bsp: Bsp,
    pub player_start: Vec3,
    pub player_yaw: f32,
    pub goals: Vec<Goal>,
    pub lights: Vec<LightSpec>,
    pub enemies: Vec<EnemySpawn>,
    pub movers: Vec<MoverSpec>,
    pub markers: Vec<MarkerSpec>,
    pub zones: Vec<ZoneSpec>,
    pub names: LevelNames,
}

impl Arena {
    /// Compiles the brushes and places every entity. The first `PlayerStart`
    /// wins; at least one goal is required. Never panics on hostile levels.
    pub fn from_level(level: &Level) -> Result<Self, LoadError> {
        Self::build(level, true)
    }

    /// The same for a level with a script, which needs no goal because the
    /// script can end the level itself.
    pub fn from_scripted_level(level: &Level) -> Result<Self, LoadError> {
        Self::build(level, false)
    }

    fn build(level: &Level, goal_required: bool) -> Result<Self, LoadError> {
        level.check_labels().map_err(LoadError::Labels)?;
        let bsp = compile(&level.static_brushes()).map_err(LoadError::Csg)?;
        let mut start = None;
        let (mut goals, mut lights, mut enemies) = (Vec::new(), Vec::new(), Vec::new());
        let (mut markers, mut zones) = (Vec::new(), Vec::new());
        // What each entity became, for its label to refer to.
        let mut things = vec![None; level.entities.len()];
        for (i, e) in level.entities.iter().enumerate() {
            if !e.is_valid() {
                return Err(LoadError::InvalidEntity(i));
            }
            match *e {
                Entity::PlayerStart { pos, yaw } => {
                    check_open(&bsp, pos, i)?;
                    start.get_or_insert((pos, yaw));
                }
                Entity::Light {
                    pos,
                    color,
                    radius,
                    intensity,
                } => lights.push(LightSpec {
                    pos,
                    color,
                    radius,
                    intensity,
                }),
                Entity::Enemy { pos, kind } => {
                    check_open(&bsp, pos, i)?;
                    things[i] = Some(Thing::Enemy(enemies.len()));
                    enemies.push(EnemySpawn { pos, kind });
                }
                Entity::Goal { pos, radius } => goals.push(Goal { pos, radius }),
                // Look settings only; the renderer reads them.
                Entity::World { .. } => {}
                Entity::Marker { pos, yaw } => {
                    things[i] = Some(Thing::Marker(markers.len()));
                    markers.push(MarkerSpec { pos, yaw });
                }
                Entity::Zone { pos, half } => {
                    things[i] = Some(Thing::Zone(zones.len()));
                    zones.push(ZoneSpec { pos, half });
                }
            }
        }
        let (player_start, player_yaw) = start.ok_or(LoadError::NoPlayerStart)?;
        if goal_required && goals.is_empty() {
            return Err(LoadError::NoGoal);
        }
        Ok(Self {
            bsp,
            player_start,
            player_yaw,
            goals,
            lights,
            enemies,
            movers: movers_of(level),
            markers,
            zones,
            names: names_of(level, &things),
        })
    }
}

/// `check_labels` has already made these unique and in range. Names on things
/// scripts have no use for, such as lights, are not listed.
fn names_of(level: &Level, things: &[Option<Thing>]) -> LevelNames {
    let mut names = LevelNames::default();
    for label in &level.labels {
        let Some(&Some(thing)) = things.get(label.entity as usize) else {
            continue;
        };
        if let Some(name) = &label.name {
            names.by_name.insert(name.clone(), thing);
        }
        for tag in &label.tags {
            names.tagged.entry(tag.clone()).or_default().push(thing);
        }
    }
    for (i, group) in level.mover_groups.iter().enumerate() {
        names.by_name.insert(group.name.clone(), Thing::Mover(i));
        for tag in &group.tags {
            let list = names.tagged.entry(tag.clone()).or_default();
            list.push(Thing::Mover(i));
        }
    }
    names
}

/// `check_labels` has already vouched for the indices.
fn movers_of(level: &Level) -> Vec<MoverSpec> {
    let spec = |g: &mscent_world::MoverGroup| {
        let brushes = g.brushes.iter();
        let brushes = brushes.filter_map(|&b| level.brushes.get(b as usize));
        MoverSpec {
            name: g.name.clone(),
            shape: Arc::new(MoverShape::from_brushes(brushes)),
        }
    };
    level.mover_groups.iter().map(spec).collect()
}

fn check_open(bsp: &Bsp, pos: Vec3, index: usize) -> Result<(), LoadError> {
    if bsp.is_solid(pos.to_f64()) {
        Err(LoadError::SpawnInSolid(index))
    } else {
        Ok(())
    }
}
