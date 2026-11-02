//! Where volumes sit among brushes, until a volume is an entry in the brush list
//! itself (M11.7). The list decides a brush's order, so a volume's stored order
//! means nothing once the list changes; what lasts is which brushes came first.

use std::collections::HashMap;
use std::sync::Arc;

use mscent_world::voxel::Volume;

use super::document::{BrushDef, Id};

/// A volume and how many brushes of the loaded order come before it.
#[derive(Clone, Debug, PartialEq)]
struct Placed {
    volume: Volume,
    rank: usize,
}

/// The volumes of a document.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Volumes {
    placed: Vec<Placed>,
    /// Brush ids in loaded order, which is what a rank counts.
    loaded: Arc<[Id]>,
}

/// The CSG order of a whole document, from [`Volumes::orders`].
pub struct CsgOrders {
    /// The order of each brush, by list position.
    pub brushes: Vec<u32>,
    /// The volumes, each with its order set.
    pub volumes: Vec<Volume>,
}

impl Volumes {
    /// `orders` holds the order of each of `brushes`, ascending as they are.
    pub fn new(volumes: &[Volume], orders: &[u32], brushes: &[BrushDef]) -> Self {
        if volumes.is_empty() {
            return Self::default();
        }
        let placed = volumes.iter().map(|v| Placed {
            volume: v.clone(),
            rank: orders.partition_point(|&o| o < v.order()),
        });
        Self {
            placed: placed.collect(),
            loaded: brushes.iter().map(|b| b.id).collect(),
        }
    }

    /// Numbers brushes and volumes as one sequence. Each volume comes right after
    /// the last surviving brush that preceded it when loaded. This depends only
    /// on the list, so deleting a brush and undoing that restores it exactly.
    /// With no volumes a brush's order is its list position.
    pub fn orders(&self, brushes: &[BrushDef]) -> CsgOrders {
        let mut places = self.places(brushes);
        places.sort_by_key(|&(place, v)| (place, v.volume.order()));
        let mut next = places.iter().map(|&(place, _)| place).peekable();
        let mut passed = 0u32;
        let brush_orders = (0..brushes.len())
            .map(|i| {
                while next.next_if(|&place| place <= i).is_some() {
                    passed += 1;
                }
                i as u32 + passed
            })
            .collect();
        let volumes = places
            .iter()
            .enumerate()
            .map(|(j, &(place, v))| v.volume.clone().with_order((place + j) as u32))
            .collect();
        CsgOrders {
            brushes: brush_orders,
            volumes,
        }
    }

    /// How many brushes of the current list come before each volume.
    fn places(&self, brushes: &[BrushDef]) -> Vec<(usize, &Placed)> {
        if self.placed.is_empty() {
            return Vec::new();
        }
        let index: HashMap<Id, usize> = brushes.iter().zip(0..).map(|(b, i)| (b.id, i)).collect();
        // ends[k] is one past the last current position among the first k loaded.
        let mut ends = vec![0usize; self.loaded.len() + 1];
        for (k, id) in self.loaded.iter().enumerate() {
            let here = index.get(id).map_or(0, |&i| i + 1);
            ends[k + 1] = ends[k].max(here);
        }
        let place = |v: &Placed| ends.get(v.rank).copied().unwrap_or(0);
        self.placed.iter().map(|v| (place(v), v)).collect()
    }
}
