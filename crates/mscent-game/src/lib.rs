//! 6DoF player, enemies, goal and the fixed-timestep loop (M6).
//!
//! # Public API
//!
//! Headless and deterministic: the same level and the same input trace give
//! the same result, bit for bit. Only `hud.rs` touches `mscent_render`.
//!
//! * [`Arena::from_level`] (`arena.rs`): compiles a `Level` and places its
//!   entities; [`LoadError`] says why a level cannot be played.
//! * [`Game`] (`game.rs`): `step(&Input)`, `update(elapsed_ns, &Input)`,
//!   `run_trace`, `restart`, `take_events`; [`Phase`] is `Playing`, `Won` or
//!   `Lost`.
//! * [`Player`] (`player.rs`): quaternion 6DoF body with inertia: the ship.
//! * [`Pilot`] (`pilot.rs`): the pilot on foot, walking, jumping and on a
//!   jetpack; `craft.rs` lands, leaves and boards the ship, and says which body
//!   is played ([`Game::actor_pos`], [`Game::aim`]). [`ScaleGuide`]
//!   (`scale.rs`) gives the sizes a level must leave open for each.
//! * [`Enemy`], [`AiState`] (`enemy.rs`): patrol, chase, attack.
//! * [`hitscan`], [`line_of_sight`], [`ray`] (`combat.rs`): traces shared by
//!   weapon, enemy sight and enemy fire. [`Weapon`] (`weapon.rs`).
//! * [`fx::Fx`] (`fx/`): particle simulation, a 60 Hz tick with pools,
//!   emitters and three collision levels. `Game` never reads it.
//! * [`Mover`], [`WhenBlocked`] (`mover.rs`): named poses, eased motion and what
//!   happens when a body is in the way; driven through `Game`.
//! * Level scripts (`script.rs`): [`Game::load_script`] compiles `mscent_script`
//!   sources, and `script_api.rs` and `script_world.rs` are the game functions
//!   they can call, all in one table, [`natives`].
//! * [`HudModel`], [`draw_hud`], [`draw_menu`] (`hud.rs`); [`Menu`]
//!   (`menu.rs`).
//!
//! World convention: camera space of `mscent_render` (+X right, +Y up, view
//! along -Z). Player yaw 0 looks along -Z and positive yaw turns left.

#![forbid(unsafe_code)]

pub mod arena;
pub mod combat;
mod craft;
pub mod destruct;
pub mod enemy;
mod facing;
pub mod fx;
pub mod game;
pub mod hud;
pub mod input;
pub mod kind;
pub mod menu;
pub mod mover;
pub mod pilot;
pub mod player;
pub mod prop;
pub mod scale;
pub mod script;
mod script_api;
mod script_props;
pub mod script_test_api;
mod script_world;
pub mod weapon;

pub use arena::{
    Arena, EnemySpawn, Goal, LevelNames, LightSpec, LoadError, MarkerSpec, MoverSpec, Thing,
    ZoneSpec,
};
pub use combat::{Hitscan, Sphere, hitscan, line_of_sight, ray};
pub use enemy::{AiState, Enemy, EnemyStats, Faction, stats_for};
pub use game::{Event, Game, Phase, Shooter, Struck};
pub use hud::{CraftHud, HudModel, draw_hud, draw_menu};
pub use input::Input;
pub use kind::{KindPlay, kinds_of, stats_of};
pub use menu::{Menu, MenuAction, MenuCommand, Screen};
pub use mover::{Mover, MoverError, WhenBlocked};
pub use pilot::Pilot;
pub use player::Player;
pub use prop::{Prop, PropMesh};
pub use scale::ScaleGuide;
pub use script_api::natives;
pub use script_test_api::Check;

/// The game functions only a test script gets, for `--script-api`.
pub fn test_natives() -> &'static [(&'static str, mscent_script::Native<Game>)] {
    script_test_api::NATIVES
}
pub use weapon::{Shot, Weapon};

#[cfg(test)]
mod adv_tests;
#[cfg(test)]
mod destruct_tests;
#[cfg(test)]
mod enemy_tests;
#[cfg(test)]
mod game_tests;
#[cfg(test)]
mod hud_tests;
#[cfg(test)]
mod kind_tests;
#[cfg(test)]
mod mover_tests;
#[cfg(test)]
mod pilot_tests;
#[cfg(test)]
mod player_tests;
#[cfg(test)]
mod script_error_tests;
#[cfg(test)]
mod script_prop_tests;
#[cfg(test)]
mod script_tests;
#[cfg(test)]
mod test_util;
#[cfg(test)]
mod voxel_tests;
