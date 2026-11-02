//! What a game looks like at a moment between two steps, for the renderer.

use mscent_game::{Faction, Game, PropMesh};
use mscent_render::{Actor, ActorKind, MoverPose};

/// The meshes to draw, `alpha` of the way through the last step.
pub(crate) fn actors_of(game: &Game, alpha: f32) -> Vec<Actor> {
    let eye = game.player.pos();
    let spin = game.ticks as f32 * 0.02;
    let goals = game.arena.goals.iter().map(|g| Actor {
        kind: ActorKind::Goal,
        pos: g.pos,
        yaw: spin,
    });
    let enemies = game.enemies.iter().filter(|e| e.alive()).map(|e| {
        let pos = e.body.prev_pos.lerp(e.body.pos, alpha);
        let to = eye - pos;
        Actor {
            kind: if e.faction == Faction::Friendly {
                ActorKind::Friendly
            } else {
                ActorKind::Enemy
            },
            pos,
            yaw: to.y.atan2(to.x) - core::f32::consts::FRAC_PI_2,
        }
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
    goals.chain(enemies).chain(props).collect()
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
