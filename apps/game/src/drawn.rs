//! What a game looks like at a moment between two steps, for the renderer.

use mscent_game::{Faction, Game, PropMesh};
use mscent_math::Vec3;
use mscent_render::model::ModelInstance;
use mscent_render::{Actor, ActorKind, LevelAssets, MoverPose};

/// What a friendly enemy's light is multiplied by: its kind's design, in green,
/// so the player can tell the sides apart on either renderer.
const FRIENDLY_TINT: Vec3 = Vec3::new(0.45, 1.0, 0.5);

/// Each live enemy as its kind's model, `alpha` of the way through the last
/// step, turned the way it faces.
pub(crate) fn enemy_models(game: &Game, alpha: f32, assets: &LevelAssets) -> Vec<ModelInstance> {
    let live = game.enemies.iter().filter(|e| e.alive());
    let placed = live.filter_map(|e| {
        let pos = e.body.prev_pos.lerp(e.body.pos, alpha);
        let mut m = ModelInstance::new(assets.kind_model(e.kind)?, pos, e.facing);
        if e.faction == Faction::Friendly {
            m.tint = FRIENDLY_TINT;
        }
        Some(m)
    });
    placed.collect()
}

/// The meshes to draw: goals and props. Enemies are models, [`enemy_models`].
pub(crate) fn actors_of(game: &Game) -> Vec<Actor> {
    let spin = game.ticks as f32 * 0.02;
    let goals = game.arena.goals.iter().map(|g| Actor {
        kind: ActorKind::Goal,
        pos: g.pos,
        yaw: spin,
    });
    let props = game.props.iter().filter(|p| p.present).map(|p| Actor {
        kind: match p.mesh {
            PropMesh::Switch => ActorKind::Switch,
            PropMesh::Keycard => ActorKind::Keycard,
            PropMesh::Terminal => ActorKind::Terminal,
            PropMesh::Crate => ActorKind::Crate,
        },
        pos: p.pos,
        yaw: spin,
    });
    goals.chain(props).collect()
}

/// Where each mover is, by its order in the level.
pub(crate) fn mover_poses_of(game: &Game, alpha: f32) -> Vec<MoverPose> {
    let movers = game.movers.iter();
    movers
        .map(|m| {
            let pose = m.drawn_pose(alpha);
            MoverPose {
                rot: pose.rot,
                pivot: pose.pivot.to_f32(),
                offset: pose.offset.to_f32(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mscent_game::{Arena, Faction, Game};
    use mscent_math::{Quat, Vec3};
    use mscent_render::{BuildConfig, LevelRenderer, MaterialTable};
    use mscent_world::{Entity, Level, LevelBuilder};

    use super::{FRIENDLY_TINT, enemy_models};

    fn setup() -> (Game, LevelRenderer) {
        let mut b = LevelBuilder::new();
        b.room([0.0, 0.0, 0.0], [12.0, 10.0, 6.0], (0, 1, 2));
        let enemy = |x: f32, kind| Entity::Enemy {
            pos: Vec3::new(x, 5.0, 3.0),
            kind,
        };
        let level = Level {
            brushes: b.finish(),
            entities: vec![
                Entity::PlayerStart {
                    pos: Vec3::new(1.0, 5.0, 3.0),
                    yaw: 0.0,
                },
                Entity::Goal {
                    pos: Vec3::new(11.0, 9.0, 3.0),
                    radius: 0.5,
                },
                enemy(5.0, 0),
                enemy(8.0, 1),
            ],
            ..Default::default()
        };
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
        let materials = MaterialTable::load_dir(&dir).expect("textures");
        let cfg = BuildConfig {
            luxel_size: 4.0,
            shadow_samples: 1,
            ..BuildConfig::default()
        };
        let renderer = LevelRenderer::from_level(&level, materials, &cfg).expect("builds");
        (
            Game::new(Arena::from_level(&level).expect("loads")),
            renderer,
        )
    }

    #[test]
    fn each_live_enemy_is_its_kinds_model_turned_as_it_faces() {
        let (mut game, r) = setup();
        let turn = Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 1.0);
        game.enemies[1].facing = turn;
        let models = enemy_models(&game, 1.0, r.assets());
        let ids: Vec<_> = models.iter().map(|m| m.model).collect();
        assert_eq!(
            ids,
            [r.assets().kind_model(0), r.assets().kind_model(1)].map(Option::unwrap)
        );
        assert_eq!(models[1].rot, turn);
        assert_eq!(models[0].pos, game.enemies[0].pos());
        assert_eq!(models[0].tint, Vec3::new(1.0, 1.0, 1.0));
    }

    #[test]
    fn a_friendly_enemy_is_tinted_and_a_dead_one_is_gone() {
        let (mut game, r) = setup();
        game.enemies[0].faction = Faction::Friendly;
        game.enemies[1].damage(i32::MAX);
        let models = enemy_models(&game, 1.0, r.assets());
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].tint, FRIENDLY_TINT);
    }
}
