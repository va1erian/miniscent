//! Problems with a level's vistas that are worth a mapper's attention, found
//! without a renderer (M13.6). None is an error: the level loads and plays, and
//! the cost is a view that leaks, shows a gradient or is not drawn.
//!
//! Both the editor and the game's `--check` list these, so the check is a pure
//! function of the level. A level with no vista layer and no viewport face is
//! answered without compiling anything.

use core::fmt;

use crate::csg::compile;
use crate::entity::{Entity, Level};
use crate::faces::extract_faces;
use crate::islands::Islands;
use crate::materials::{VIEW_SLOTS, view_slot};
use crate::portals::Portals;

/// One thing worth knowing about a level's vistas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VistaWarning {
    /// The layer's room joins the one the player starts in: the view is
    /// reachable, so the player can walk into it.
    SharesPlayerIsland { slot: u8, layer: u8 },
    /// The layer's `pos` is in solid, so it shows nothing.
    LayerInSolid { slot: u8, layer: u8 },
    /// A wall shows this slot, and no layer feeds it: it shows the gradient.
    SlotWithoutLayer { slot: u8 },
    /// A mover's brush has a viewport face; a mover is never drawn as a hole.
    MoverWithViewport { group: String },
}

impl fmt::Display for VistaWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SharesPlayerIsland { slot, layer } => write!(
                f,
                "vista slot {slot} layer {layer} is in the same room space as the player \
                 start, so the player can walk into it; seal it with solid"
            ),
            Self::LayerInSolid { slot, layer } => write!(
                f,
                "vista slot {slot} layer {layer} has its position in solid and shows nothing"
            ),
            Self::SlotWithoutLayer { slot } => write!(
                f,
                "a wall shows view {slot} but no vista layer has slot {slot}: it shows the gradient"
            ),
            Self::MoverWithViewport { group } => write!(
                f,
                "mover group {group:?} has a viewport face, which a mover does not draw as a view"
            ),
        }
    }
}

/// Whether any brush plane of `level` has a viewport material.
fn has_viewport_plane(level: &Level) -> bool {
    level
        .brushes
        .iter()
        .flat_map(|b| b.texinfos())
        .any(|t| view_slot(t.material).is_some())
}

/// The mover groups with a viewport face, by name.
fn movers_with_viewport(level: &Level) -> impl Iterator<Item = VistaWarning> + '_ {
    level.mover_groups.iter().filter_map(|g| {
        let has = g.brushes.iter().any(|&i| {
            let brush = level.brushes.get(i as usize);
            let mut planes = brush.into_iter().flat_map(|b| b.texinfos());
            planes.any(|t| view_slot(t.material).is_some())
        });
        has.then(|| VistaWarning::MoverWithViewport {
            group: g.name.clone(),
        })
    })
}

fn player_start(level: &Level) -> Option<mscent_math::Vec3> {
    level.entities.iter().find_map(|e| match *e {
        Entity::PlayerStart { pos, .. } => Some(pos),
        _ => None,
    })
}

/// What is worth telling the mapper about the vistas of `level`. A level that
/// cannot be compiled has none to judge, and a bad one is reported where it is
/// built.
pub fn vista_warnings(level: &Level) -> Vec<VistaWarning> {
    let layers = level.vista_layers();
    if layers.is_empty() && !has_viewport_plane(level) {
        return Vec::new();
    }
    let mut out: Vec<VistaWarning> = movers_with_viewport(level).collect();
    let csg = level.csg_brushes();
    let Ok(bsp) = compile(&csg) else {
        return out;
    };
    let islands = Islands::build(&bsp, &Portals::build(&bsp));
    let home = player_start(level).and_then(|p| islands.island_of(bsp.leaf_at_f32(p)));
    let mut fed = [false; VIEW_SLOTS as usize];
    for entity in layers.iter().filter(|e| e.is_valid()) {
        let Entity::VistaLayer {
            pos, slot, layer, ..
        } = **entity
        else {
            continue;
        };
        if let Some(seen) = fed.get_mut(usize::from(slot)) {
            *seen = true;
        }
        let Some(island) = islands.island_of(bsp.leaf_at_f32(pos)) else {
            out.push(VistaWarning::LayerInSolid { slot, layer });
            continue;
        };
        if Some(island) == home {
            out.push(VistaWarning::SharesPlayerIsland { slot, layer });
        }
    }
    let mut shown = [false; VIEW_SLOTS as usize];
    for face in extract_faces(&bsp, &csg) {
        if let Some(slot) = view_slot(face.material) {
            shown[usize::from(slot)] = true;
        }
    }
    for slot in 0..VIEW_SLOTS {
        if shown[usize::from(slot)] && !fed[usize::from(slot)] {
            out.push(VistaWarning::SlotWithoutLayer { slot });
        }
    }
    out
}

#[cfg(test)]
#[path = "vista_check_tests.rs"]
mod tests;
