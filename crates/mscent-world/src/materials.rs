//! The stock material ids shared by the level builders and the renderer.
//!
//! `assets/textures/materials.txt` decides every id, and may list more materials
//! after these. It must keep these five where they are: the renderer checks the
//! names when it builds a level, so a mismatch is reported there.

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

/// Stock texture names in id order.
pub const NAMES: [&str; 5] = ["brick", "floor", "lightpanel", "metal", "rock"];
