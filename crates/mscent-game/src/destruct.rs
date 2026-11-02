//! Shots carving destructible voxel rock.
//!
//! Collision must be a function of the tick, or a replay would diverge, so all of
//! this runs on the fixed step and none of it on a background thread. A dab's
//! chunks are meshed off to the side, at most [`MESHES_PER_TICK`] a tick and first
//! come first served, and swapped in together on the tick the last one finishes.
//! Collision and drawing take the same swap, so they never disagree and no frame
//! shows half a hole.
//!
//! Only material goes, which cannot embed anything, and a dab never touches the
//! ring around a volume, so no hole can reach the void. Damage is not saved: a
//! restart goes back to the level's own data.

use std::collections::VecDeque;

use mscent_math::{Vec3, Vec3d};
use mscent_world::Bsp;
use mscent_world::brush::Brush;
use mscent_world::materials::is_destructible;
use mscent_world::voxel::{
    Context, DabOp, Prim, RemeshedChunk, Volume, VoxelWorld, apply_dab, solid_material_near,
};

use crate::arena::Arena;
use crate::game::Game;
use crate::weapon::Shot;

/// Radius of the hole a shot makes, in world units.
pub const CARVE_RADIUS: f64 = 0.6;
/// Chunks meshed in one 120 Hz tick.
pub const MESHES_PER_TICK: usize = 2;
/// Dabs waiting at most. A hit past this leaves no mark, rather than a backlog
/// that would show up seconds after the shots that made it.
const MAX_QUEUED: usize = 16;

/// A change the renderer must take to match collision.
#[derive(Clone, Debug, PartialEq)]
pub enum VoxelUpdate {
    /// These chunks are new.
    Chunks(Vec<RemeshedChunk>),
    /// All surfaces are back as the level has them.
    Reset(Box<VoxelWorld>),
}

/// A dab on its way to the world.
#[derive(Clone, Debug, PartialEq)]
struct Queued {
    volume: usize,
    at: Vec3d,
    /// Filled when the dab reaches the head of the queue, as only then is the
    /// volume it writes to the one it should see. A touching volume reads this
    /// one's samples through its ring, so some of these may be its chunks.
    chunks: Option<Vec<(usize, [u16; 3])>>,
    next: usize,
    done: Vec<RemeshedChunk>,
}

/// What destruction keeps from one tick to the next.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Destruction {
    /// The volumes as damage has left them. Empty until the first hit.
    volumes: Vec<Volume>,
    queue: VecDeque<Queued>,
    /// The surfaces as the level has them, from the first hit on.
    pristine: Option<VoxelWorld>,
    updates: Vec<VoxelUpdate>,
    meshed_last_tick: usize,
}

/// The level data destruction reads, which the arena owns.
#[derive(Clone, Copy)]
pub struct VoxelSource<'a> {
    pub volumes: &'a [Volume],
    pub brushes: &'a [Brush],
    pub bsp: &'a Bsp,
}

impl Destruction {
    /// Queues a carve if a shot ending at `point` struck destructible rock.
    pub fn hit(&mut self, source: VoxelSource, world: &VoxelWorld, point: Vec3) -> bool {
        if self.queue.len() >= MAX_QUEUED {
            return false;
        }
        let at = point.to_f64();
        if self.volumes.is_empty() {
            self.volumes = source.volumes.to_vec();
        }
        let Some(volume) = (0..self.volumes.len()).find(|&v| {
            let current = &self.volumes[v];
            current.holds(at) && struck_destructible(current, source.brushes, at)
        }) else {
            return false;
        };
        self.pristine.get_or_insert_with(|| world.clone());
        self.queue.push_back(Queued {
            volume,
            at,
            chunks: None,
            next: 0,
            done: Vec::new(),
        });
        true
    }

    /// One tick's meshing. A finished dab goes into `world`.
    pub fn step(&mut self, source: VoxelSource, world: &mut VoxelWorld) {
        let mut budget = MESHES_PER_TICK;
        self.meshed_last_tick = 0;
        while budget > 0 {
            let Some(head) = self.queue.front_mut() else {
                return;
            };
            if head.chunks.is_none() {
                head.chunks = Some(carve(&mut self.volumes, head, source.brushes));
            }
            let chunks = head.chunks.as_deref().unwrap_or(&[]);
            let take = budget.min(chunks.len() - head.next);
            let wanted: Vec<(usize, Vec<[u16; 3]>)> = chunks[head.next..head.next + take]
                .iter()
                .map(|&(v, c)| (v, vec![c]))
                .collect();
            head.done.extend(VoxelWorld::remesh(
                &self.volumes,
                source.brushes,
                source.bsp,
                &wanted,
            ));
            head.next += take;
            budget -= take;
            self.meshed_last_tick += take;
            if head.next == chunks.len() {
                let done = std::mem::take(&mut head.done);
                self.queue.pop_front();
                world.apply(done.clone());
                self.updates.push(VoxelUpdate::Chunks(done));
            }
        }
    }

    /// Puts the surfaces back as the level has them, if anything has changed.
    pub fn restore(&mut self, world: &mut VoxelWorld) {
        self.volumes.clear();
        self.queue.clear();
        self.updates.clear();
        if let Some(pristine) = self.pristine.take() {
            *world = pristine.clone();
            self.updates.push(VoxelUpdate::Reset(Box::new(pristine)));
        }
    }

    /// The changes since the last call, in order.
    pub fn take_updates(&mut self) -> Vec<VoxelUpdate> {
        std::mem::take(&mut self.updates)
    }

    pub fn meshed_last_tick(&self) -> usize {
        self.meshed_last_tick
    }

    pub fn is_idle(&self) -> bool {
        self.queue.is_empty()
    }

    /// The volumes as damage has left them, once anything has been hit.
    pub fn damaged_volumes(&self) -> &[Volume] {
        &self.volumes
    }
}

fn struck_destructible(volume: &Volume, brushes: &[Brush], at: Vec3d) -> bool {
    let mut ctx = Context::new(volume, brushes);
    solid_material_near(volume, &mut ctx, at).is_some_and(is_destructible)
}

fn carve(volumes: &mut [Volume], head: &Queued, brushes: &[Brush]) -> Vec<(usize, [u16; 3])> {
    let Some(volume) = volumes.get_mut(head.volume) else {
        return Vec::new();
    };
    let Some(prim) = Prim::sphere(head.at, CARVE_RADIUS) else {
        return Vec::new();
    };
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, brushes);
    let own = apply_dab(volume, &mut ctx, &prim, DabOp::Carve);
    let mut out: Vec<(usize, [u16; 3])> = own.into_iter().map(|c| (head.volume, c)).collect();
    let (lo, hi) = prim.bounds();
    for (i, other) in volumes.iter().enumerate() {
        if i != head.volume && other.touches(&shape) {
            out.extend(other.chunks_near(lo, hi).into_iter().map(|c| (i, c)));
        }
    }
    out
}

impl Game {
    /// What the renderer must take to match collision, since the last call.
    pub fn take_voxel_updates(&mut self) -> Vec<VoxelUpdate> {
        self.destruct.take_updates()
    }

    /// Takes the shot-up surfaces back to the level's, leaving the rest as it is:
    /// damage belongs to a run, not to what the editor shows after it.
    pub fn restore_voxels(&mut self) {
        self.destruct.restore(&mut self.arena.voxels);
    }

    fn voxel_source(arena: &Arena) -> VoxelSource<'_> {
        VoxelSource {
            volumes: &arena.volumes,
            brushes: &arena.static_brushes,
            bsp: &arena.bsp,
        }
    }

    /// A shot that ended on a wall, not on anything that moves, may have broken it.
    pub(crate) fn carve_struck(&mut self, shot: &Shot) {
        let on_wall = shot.hit.is_none() && shot.prop.is_none() && shot.mover.is_none();
        if on_wall && shot.normal != Vec3::ZERO && !self.arena.volumes.is_empty() {
            let source = Self::voxel_source(&self.arena);
            self.destruct.hit(source, &self.arena.voxels, shot.point);
        }
    }

    pub(crate) fn step_destruction(&mut self) {
        if self.destruct.is_idle() {
            return;
        }
        // The world is borrowed apart from what the dab reads.
        let mut world = std::mem::take(&mut self.arena.voxels);
        self.destruct
            .step(Self::voxel_source(&self.arena), &mut world);
        self.arena.voxels = world;
    }
}
