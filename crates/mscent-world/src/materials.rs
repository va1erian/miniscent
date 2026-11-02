//! The stock material ids shared by the level builders and the renderer.
//!
//! `assets/textures/materials.txt` decides every id, and may list more materials
//! after these. It must keep the five in [`NAMES`] where they are: the renderer
//! checks those names when it builds a level, so a mismatch is reported there.
//! The living materials the voxel generators grow come after them, and their ids
//! are held to the manifest by a test.

/// Red brick walls.
pub const BRICK: u16 = 0;
/// Tiled hazard floor.
pub const FLOOR: u16 = 1;
/// Emissive ceiling light panel.
pub const LIGHTPANEL: u16 = 2;
/// Riveted steel plates.
pub const METAL: u16 = 3;
/// Rough cave rock.
pub const ROCK: u16 = 4;

/// Furrowed tree bark.
pub const BARK: u16 = 5;
/// A toadstool's red cap; its spots come from the renderer, not the texture.
pub const CAP: u16 = 6;
/// Rose coral pitted with polyp cups.
pub const CORAL: u16 = 7;
/// A mushroom's pale, fibrous stem.
pub const STEM: u16 = 8;
/// Honey-coloured comb wax.
pub const WAX: u16 = 9;

/// A face with this material is never drawn or lit: the shell of a voxel volume,
/// where the voxel surface closes in front of it. Not a texture.
pub const NO_DRAW: u16 = u16::MAX;

/// Whether a shot carves voxel rock of this material. Rock breaks; the made
/// materials do not. A table of the game's own materials, not level data.
pub const fn is_destructible(material: u8) -> bool {
    material as u16 == ROCK
}

/// Stock texture names in id order.
pub const NAMES: [&str; 5] = ["brick", "floor", "lightpanel", "metal", "rock"];
