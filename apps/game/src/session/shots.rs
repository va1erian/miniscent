//! A shot as the game reported it, turned into an effect: where the bolt
//! starts and what it does where it ends.

use mscent_game::fx::combat::{Impact, ShotFx};
use mscent_game::{Shooter, Struck};
use mscent_math::Vec3;

use super::GameSession;

/// Where the player's bolt starts: right of and below the eye, and ahead of
/// it, in the camera frame (+X right, +Y up, view along -Z).
const GUN_OFFSET: Vec3 = Vec3::new(0.22, -0.18, -0.35);

impl GameSession {
    pub(super) fn shot_effect(
        &mut self,
        shooter: Shooter,
        origin: Vec3,
        end: Vec3,
        normal: Vec3,
        struck: Struck,
    ) {
        let by_player = shooter == Shooter::Player;
        let muzzle = if by_player {
            origin + self.game.player.orientation.rotate(GUN_OFFSET)
        } else {
            origin
        };
        let impact = match struck {
            // A hit on the player would burst inside the camera.
            Struck::Nothing | Struck::Player => Impact::None,
            Struck::Enemy(_) => Impact::Drone,
            Struck::Wall => {
                self.renderer
                    .scene()
                    .surface_at(end, normal)
                    .map_or(Impact::None, |s| Impact::Surface {
                        material: s.material,
                        tint: s.tint,
                    })
            }
        };
        self.fx.shot(&ShotFx {
            by_player,
            muzzle,
            end,
            normal,
            impact,
        });
    }
}
