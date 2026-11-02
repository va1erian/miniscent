//! A shot as the game reported it, turned into an effect: where the bolt
//! starts and what it does where it ends.

use mscent_game::fx::combat::{Impact, ShotFx, enemy_muzzle};
use mscent_game::fx::decal::FaceOutline;
use mscent_game::{Shooter, Struck};
use mscent_math::Vec3;
use mscent_world::materials::METAL;

use super::GameSession;

/// Where the player's bolt starts: right of and below the eye, and ahead of
/// it, in the camera frame (+X right, +Y up, view along -Z).
const GUN_OFFSET: Vec3 = Vec3::new(0.22, -0.18, -0.35);
/// What a door or a prop is taken to be made of. A surface lookup walks the
/// world's leaves, which hold neither.
const MOVABLE_TINT: u32 = 0xffa0_a8b0;

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
            enemy_muzzle(origin, end)
        };
        let impact = match struck {
            // A hit on the player would burst inside the camera.
            Struck::Nothing | Struck::Player => Impact::None,
            Struck::Enemy(_) => Impact::Drone,
            Struck::Mover(_) | Struck::Prop(_) => Impact::Surface {
                material: METAL,
                tint: MOVABLE_TINT,
            },
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
        // Only a wall has a face for the scorch to stay on.
        let face = match struck {
            Struck::Wall => self.renderer.scene().face_at(end, normal),
            _ => None,
        };
        let outline = face.and_then(FaceOutline::of);
        self.fx.shot_on(
            &ShotFx {
                by_player,
                muzzle,
                end,
                normal,
                impact,
            },
            outline.as_ref(),
        );
    }
}
