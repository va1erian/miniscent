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
//! * [`Player`] (`player.rs`): quaternion 6DoF body with inertia.
//! * [`Enemy`], [`AiState`] (`enemy.rs`): patrol, chase, attack.
//! * [`hitscan`], [`line_of_sight`], [`ray`] (`combat.rs`): traces shared by
//!   weapon, enemy sight and enemy fire. [`Weapon`] (`weapon.rs`).
//! * [`HudModel`], [`draw_hud`], [`draw_menu`] (`hud.rs`); [`Menu`]
//!   (`menu.rs`).
//!
//! World convention: camera space of `mscent_render` (+X right, +Y up, view
//! along -Z). Player yaw 0 looks along -Z and positive yaw turns left.

pub mod arena;
pub mod combat;
pub mod enemy;
pub mod game;
pub mod hud;
pub mod input;
pub mod menu;
pub mod player;
pub mod weapon;

pub use arena::{Arena, EnemySpawn, Goal, LightSpec, LoadError};
pub use combat::{Hitscan, Sphere, hitscan, line_of_sight, ray};
pub use enemy::{AiState, Enemy, EnemyStats, stats_for};
pub use game::{Event, Game, Phase};
pub use hud::{HudModel, draw_hud, draw_menu};
pub use input::Input;
pub use menu::{Menu, MenuAction, MenuCommand, Screen};
pub use player::Player;
pub use weapon::{Shot, Weapon};

#[cfg(test)]
mod adv_tests;
#[cfg(test)]
mod enemy_tests;
#[cfg(test)]
mod game_tests;
#[cfg(test)]
mod hud_tests;
#[cfg(test)]
mod player_tests;
#[cfg(test)]
mod test_util;
