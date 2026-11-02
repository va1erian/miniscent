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

/// The material of viewport slot 0. A face with `VIEW_FIRST + slot` collides like
/// any wall but is drawn as a hole onto that slot's vista, not as a texture.
pub const VIEW_FIRST: u16 = 0xFFF0;
/// How many viewport slots there are, so ids `VIEW_FIRST..VIEW_FIRST + VIEW_SLOTS`.
pub const VIEW_SLOTS: u8 = 8;

/// The viewport slot a material id stands for, or `None` for a texture and for
/// [`NO_DRAW`], which sits just above the slots.
pub const fn view_slot(material: u16) -> Option<u8> {
    if material >= VIEW_FIRST && material - VIEW_FIRST < VIEW_SLOTS as u16 {
        Some((material - VIEW_FIRST) as u8)
    } else {
        None
    }
}

/// The material id of viewport `slot`, or `None` when the slot is out of range.
pub const fn view_material(slot: u8) -> Option<u16> {
    if slot < VIEW_SLOTS {
        Some(VIEW_FIRST + slot as u16)
    } else {
        None
    }
}

/// Whether a shot carves voxel rock of this material. Rock breaks; the made
/// materials do not. A table of the game's own materials, not level data.
pub const fn is_destructible(material: u8) -> bool {
    material as u16 == ROCK
}

/// Stock texture names in id order.
pub const NAMES: [&str; 5] = ["brick", "floor", "lightpanel", "metal", "rock"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_slots_cover_exactly_eight_ids_below_no_draw() {
        assert_eq!(view_slot(0xFFEF), None);
        assert_eq!(view_slot(0xFFF0), Some(0));
        assert_eq!(view_slot(0xFFF7), Some(7));
        assert_eq!(view_slot(0xFFF8), None);
        assert_eq!(view_slot(NO_DRAW), None);
        assert_eq!(view_slot(BRICK), None);
        for slot in 0..VIEW_SLOTS {
            assert_eq!(view_slot(view_material(slot).unwrap()), Some(slot));
        }
        assert_eq!(view_material(VIEW_SLOTS), None);
    }
}
