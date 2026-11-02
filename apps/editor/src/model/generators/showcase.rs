//! The showcase levels, one per use case, built the way the editor builds
//! them: generators give plans, plans go through the dab code. They are only
//! for tests and for the pictures `voxel_shot` takes of the saved files.

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::materials::{BRICK, ROCK};
use mscent_world::voxel::{
    CHUNK_EDGE, CellSize, Context, DabOp, Prim, Volume, apply_dab, first_solid,
};
use mscent_world::{Brush, Entity, Level, Op};

use super::{Kind, Plan, Site, generate, surface_normal};
use crate::model::pick::Ray;

const ROCK_ID: u8 = ROCK as u8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Case {
    Hive,
    Coral,
    Mushrooms,
    Tree,
    Cave,
}

impl Case {
    pub const ALL: [Case; 5] = [
        Case::Hive,
        Case::Coral,
        Case::Mushrooms,
        Case::Tree,
        Case::Cave,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Case::Hive => "hive",
            Case::Coral => "coral",
            Case::Mushrooms => "mushrooms",
            Case::Tree => "tree",
            Case::Cave => "cave",
        }
    }

    /// The saved file, in `assets/levels`.
    pub fn file(self) -> String {
        format!("voxel-{}.msl", self.name())
    }
}

/// A level and a pose in its air to look at it from.
pub struct Scene {
    pub level: Level,
    pub eye: Vec3,
    pub target: Vec3,
}

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn room(lo: Vec3d, hi: Vec3d) -> Brush {
    Brush::cuboid(lo, hi, Op::Subtract, 0)
        .expect("a valid room")
        .with_material(ROCK)
}

fn lamp(at: Vec3d, radius: f32) -> Entity {
    Entity::Light {
        pos: at.to_f32(),
        color: Vec3::new(1.0, 0.95, 0.85),
        radius,
        intensity: 1.0,
    }
}

/// An empty volume, aligned to whole chunks, that holds the box `lo..hi`.
fn volume_around(op: Op, cell: CellSize, lo: Vec3d, hi: Vec3d) -> Volume {
    let size = cell.units() * CHUNK_EDGE as f64;
    let margin = 2.0 * cell.units();
    let first = [lo.x, lo.y, lo.z].map(|c| ((c - margin) / size).floor());
    let last = [hi.x, hi.y, hi.z].map(|c| ((c + margin) / size).ceil());
    let origin = first.map(|f| f as i32 * CHUNK_EDGE as i32);
    let chunks = [0, 1, 2].map(|a| (last[a] - first[a]) as u16);
    Volume::new(op, 1, cell, origin, chunks).expect("a volume that fits")
}

fn apply_plan(volume: &mut Volume, ctx: &mut Context, plan: &Plan) {
    for piece in plan.pieces() {
        apply_dab(volume, ctx, &piece.prim, piece.op);
    }
}

fn site(cell: CellSize, origin: Vec3d, normal: Vec3d, size: f64, material: u16) -> Site {
    Site {
        origin,
        normal,
        size,
        cell: cell.units(),
        material: material as u8,
    }
}

/// A room with generators at some sites, built in one add volume of `cell`.
struct Garden {
    room: Brush,
    cell: CellSize,
    sites: Vec<(Kind, Site)>,
    eye: Vec3d,
    target: Vec3d,
}

impl Garden {
    fn new(room: Brush, cell: CellSize, eye: Vec3d, target: Vec3d) -> Self {
        Self {
            room,
            cell,
            sites: Vec::new(),
            eye,
            target,
        }
    }

    fn grow(mut self, kind: Kind, at: Vec3d, normal: Vec3d, size: f64, material: u16) -> Self {
        self.sites
            .push((kind, site(self.cell, at, normal, size, material)));
        self
    }

    fn scene(self, seed: u32) -> Scene {
        let plans: Vec<Plan> = (self.sites.iter().zip(0..))
            .map(|((kind, at), i)| generate(*kind, at, seed.wrapping_add(i)))
            .collect();
        let (lo, hi) = plans
            .iter()
            .filter_map(Plan::bounds)
            .reduce(|(a, b), (c, d)| (a.min(c), b.max(d)))
            .expect("the sites grow something");
        let brushes = vec![self.room];
        let mut volume = volume_around(Op::Add, self.cell, lo, hi);
        let shape = volume.clone();
        let mut ctx = Context::new(&shape, &brushes);
        for plan in &plans {
            apply_plan(&mut volume, &mut ctx, plan);
        }
        let hall = brushes[0].bounds();
        let (lo, hi) = (hall.min.to_f64(), hall.max.to_f64());
        let mid = v((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0, hi.z - 2.0);
        let level = Level {
            brushes,
            volumes: vec![volume],
            entities: vec![lamp(mid, 40.0)],
            ..Default::default()
        };
        Scene {
            level,
            eye: self.eye.to_f32(),
            target: self.target.to_f32(),
        }
    }
}

fn cave(seed: u32) -> Scene {
    let cell = CellSize::Quarter;
    let mut rng = Rng::new(u64::from(seed) ^ 0xCA7E);
    let brushes = vec![room(v(-6.0, -14.0, 0.0), v(6.0, -4.0, 6.0))];
    let mut volume = volume_around(
        Op::Subtract,
        cell,
        v(-10.0, -6.0, -1.0),
        v(10.0, 18.0, 11.0),
    );
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, &brushes);
    // The tunnel: a wandering chain of round cones going in through the wall.
    let (mut at, mut radius) = (v(0.0, -5.0, 2.6), 2.2);
    let mut path = vec![at];
    for _ in 0..6 {
        let next = at + v(rng.range_f64(-1.2, 1.2), 2.6, rng.range_f64(-0.4, 0.4));
        let r = (radius + rng.range_f64(-0.4, 0.4)).clamp(1.8, 2.6);
        let cone = Prim::round_cone(at, next, radius, r);
        apply_dab(&mut volume, &mut ctx, &cone.expect("valid"), DabOp::Carve);
        (at, radius) = (next, r);
        path.push(at);
    }
    let chamber = Prim::sphere(at + v(0.0, 1.0, 0.6), 3.8).expect("valid");
    apply_dab(&mut volume, &mut ctx, &chamber, DabOp::Carve);
    for p in &path {
        let bumps = Prim::sphere(*p, 3.4).expect("valid");
        let roughen = DabOp::Roughen {
            seed,
            amplitude: 9,
            material: ROCK_ID,
        };
        apply_dab(&mut volume, &mut ctx, &bumps, roughen);
    }
    // Drips from the ceiling and mounds from the floor, wherever a ray finds rock.
    for (k, p) in path.iter().skip(1).enumerate() {
        for (dir, kind, size) in [
            (v(0.0, 0.0, 1.0), Kind::Stalactites, 0.8),
            (v(0.0, 0.0, -1.0), Kind::Mushrooms, 0.6),
        ] {
            let ray = Ray { origin: *p, dir };
            let Some(t) = first_solid(&volume, &mut ctx, ray.origin, ray.dir, 12.0) else {
                continue;
            };
            let normal = surface_normal(&ray, 0.4, |r| {
                first_solid(&volume, &mut ctx, r.origin, r.dir, 12.0)
            });
            let at = site(cell, ray.origin + dir * t, normal, size, ROCK);
            let plan = generate(kind, &at, seed.wrapping_add(k as u32));
            apply_plan(&mut volume, &mut ctx, &plan);
        }
    }
    let level = Level {
        brushes,
        volumes: vec![volume],
        entities: vec![lamp(v(0.0, -9.0, 5.0), 30.0), lamp(v(0.0, 4.0, 3.5), 14.0)],
        ..Default::default()
    };
    Scene {
        level,
        eye: Vec3::new(0.0, -3.0, 2.6),
        target: Vec3::new(0.0, 8.0, 2.6),
    }
}

/// The scene for `case`: its level, and where to look at it from.
pub fn build(case: Case, seed: u32) -> Scene {
    let up = v(0.0, 0.0, 1.0);
    let hall = || room(v(-12.0, -12.0, 0.0), v(12.0, 12.0, 10.0));
    let eighth = CellSize::Eighth;
    match case {
        Case::Hive => {
            let wall = v(0.0, -1.0, 0.0);
            Garden::new(
                hall(),
                CellSize::Quarter,
                v(0.0, 3.0, 5.0),
                v(0.0, 12.0, 5.0),
            )
            .grow(Kind::Hive, v(0.0, 12.0, 5.0), wall, 1.5, ROCK)
            .grow(Kind::Hive, v(-8.5, 12.0, 4.0), wall, 1.1, ROCK)
            .scene(seed)
        }
        Case::Coral => Garden::new(hall(), eighth, v(0.0, -8.0, 3.5), v(0.0, 3.0, 1.8))
            .grow(Kind::Coral, v(-3.0, 3.0, 0.0), up, 1.4, BRICK)
            .grow(Kind::Coral, v(3.0, 1.0, 0.0), up, 1.0, BRICK)
            .grow(Kind::Coral, v(0.0, 6.0, 0.0), up, 1.2, BRICK)
            .scene(seed),
        Case::Mushrooms => Garden::new(
            hall(),
            CellSize::Quarter,
            v(0.0, -8.0, 2.5),
            v(0.0, 3.0, 1.5),
        )
        .grow(Kind::Mushrooms, v(-3.0, 3.0, 0.0), up, 1.5, BRICK)
        .grow(Kind::Mushrooms, v(3.0, 1.0, 0.0), up, 1.0, BRICK)
        .grow(Kind::Mushrooms, v(0.5, 6.0, 0.0), up, 0.8, BRICK)
        .scene(seed),
        Case::Tree => {
            let tall = room(v(-14.0, -14.0, 0.0), v(14.0, 14.0, 16.0));
            Garden::new(tall, eighth, v(0.0, -11.0, 4.0), v(0.0, 4.0, 6.0))
                .grow(Kind::Tree, v(0.0, 4.0, 0.0), up, 1.8, ROCK)
                .scene(seed)
        }
        Case::Cave => cave(seed),
    }
}
