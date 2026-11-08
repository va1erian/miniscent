//! A level's enemy kinds as models (M19.3): built when the level is, so every
//! enemy, whoever draws it, is its kind's design.
//!
//! The stock pair is the same for every level without kinds of its own, so it
//! is built once a process and shared; a level's own kinds are built with it.
//! A kind that will not build fails the level, naming the kind, as a level that
//! will not compile does.

use std::sync::{Arc, OnceLock};

use mscent_math::{Quat, Vec3};
use mscent_world::model::{self, Blueprint, Model};
use mscent_world::{Entity, Level};

use super::BuildError;
use crate::material::MaterialTable;
use crate::model::{ModelId, ModelInstance, ModelTable};

/// A design and the model built from it.
pub(super) type Design = Arc<(Blueprint, Model)>;

/// Builds each kind `level` plays, in its order.
pub(super) fn designs_of(level: &Level) -> Result<Vec<Design>, BuildError> {
    if level.kinds.is_empty() {
        static STOCK: OnceLock<Result<Vec<Design>, BuildError>> = OnceLock::new();
        return STOCK.get_or_init(|| build_all(&level.kind_table())).clone();
    }
    build_all(&level.kinds)
}

fn build_all(kinds: &[mscent_world::Kind]) -> Result<Vec<Design>, BuildError> {
    let one = |(i, k): (usize, &mscent_world::Kind)| {
        let built = model::build(&k.blueprint).map_err(|_| BuildError::Model(i))?;
        Ok(Arc::new((k.blueprint.clone(), built)))
    };
    kinds.iter().enumerate().map(one).collect()
}

/// Where the level's enemies start and of what kind.
pub(super) fn spawns_of(level: &Level) -> Vec<(Vec3, u8)> {
    let spawn = |e: &Entity| match *e {
        Entity::Enemy { pos, kind } => Some((pos, kind)),
        _ => None,
    };
    level.entities.iter().filter_map(spawn).collect()
}

/// The kinds' models, in a table that can take more, and the level's enemies
/// placed at their spawns facing +X, as they face before their first step.
pub(super) struct KindModels {
    pub table: ModelTable,
    pub ids: Vec<ModelId>,
    pub spawns: Vec<ModelInstance>,
}

impl KindModels {
    pub fn new(designs: &[Design], spawns: &[(Vec3, u8)], materials: &MaterialTable) -> Self {
        let mut table = ModelTable::new();
        let ids: Vec<ModelId> = designs
            .iter()
            .map(|d| table.add(&d.0, &d.1, materials))
            .collect();
        let placed = |&(pos, kind): &(Vec3, u8)| {
            let id = ids.get(usize::from(kind))?;
            Some(ModelInstance::new(*id, pos, Quat::IDENTITY))
        };
        let spawns = spawns.iter().filter_map(placed).collect();
        Self { table, ids, spawns }
    }
}
