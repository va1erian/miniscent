//! Islands: the sets of empty leaves that portals join (M13.1).
//!
//! A viewport layer lives in a sealed room of its own. Finding the islands once
//! lets later stages keep a layer's visibility, lamps and lightmap density inside
//! its room without asking whether two leaves are connected each time.

use crate::bsp::{Bsp, LeafId, LeafKind};
use crate::portals::Portals;

/// The islands of a compiled level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Islands {
    /// For each leaf, its island, or `NONE` for a solid leaf.
    of_leaf: Vec<u32>,
    /// Each island's leaves, in the order the flood reached them.
    members: Vec<Vec<LeafId>>,
}

const NONE: u32 = u32::MAX;

impl Islands {
    /// Floods over `portals` from each empty leaf not yet reached. Island numbers
    /// follow the lowest leaf id in each island.
    pub fn build(bsp: &Bsp, portals: &Portals) -> Self {
        let mut of_leaf = vec![NONE; bsp.leaves().len()];
        let mut members: Vec<Vec<LeafId>> = Vec::new();
        for (i, leaf) in (0u32..).zip(bsp.leaves()) {
            if leaf.kind != LeafKind::Empty || of_leaf[i as usize] != NONE {
                continue;
            }
            // Leaf counts are bounded by the BSP's u32 ids, so this cannot fail.
            let island = u32::try_from(members.len()).unwrap_or(NONE - 1);
            of_leaf[i as usize] = island;
            let mut found = vec![LeafId(i)];
            let mut head = 0;
            while let Some(&at) = found.get(head) {
                head += 1;
                for next in portals.neighbors(at) {
                    if let Some(slot) = of_leaf.get_mut(next.0 as usize)
                        && *slot == NONE
                        && bsp.leaf_kind(next) == LeafKind::Empty
                    {
                        *slot = island;
                        found.push(next);
                    }
                }
            }
            members.push(found);
        }
        Self { of_leaf, members }
    }

    /// How many islands there are.
    pub fn count(&self) -> u32 {
        u32::try_from(self.members.len()).unwrap_or(u32::MAX)
    }

    /// The island `leaf` belongs to; `None` for a solid leaf or one out of range.
    pub fn island_of(&self, leaf: LeafId) -> Option<u32> {
        self.of_leaf
            .get(leaf.0 as usize)
            .copied()
            .filter(|&i| i != NONE)
    }

    /// The leaves of `island`; empty when there is no such island.
    pub fn leaves(&self, island: u32) -> &[LeafId] {
        self.members.get(island as usize).map_or(&[], Vec::as_slice)
    }
}
