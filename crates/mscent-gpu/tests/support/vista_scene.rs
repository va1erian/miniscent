//! The level of [`Scene::vista`]: a brick room with a window onto slot 0, whose
//! far layer is a room of stars and whose near layer holds a lit block, the
//! planet and the clouds, the two stock cutouts, one soft, one hard.

use std::cell::RefCell;

use mscent_math::{Vec3, Vec3d};
use mscent_render::{BakeCache, BuildConfig, MaterialId, MaterialTable, Texture};
use mscent_world::materials::{BRICK, METAL, ROCK, SKY_CLOUDS, SKY_PLANET, SKY_STARS, VIEW_FIRST};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab};
use mscent_world::{Brush, Entity, Level, MAX_VISTA_FOG, Op};

use super::{Scene, Taken, real_materials};

/// Where the layers' rooms are, along x.
const STARS_AT: f64 = 1000.0;
const NEAR_AT: f64 = 2000.0;
/// How far the near layer's eye moves per unit the player moves.
const NEAR_PARALLAX: f32 = 0.25;
/// The stars' turn, about the up axis, in radians a second.
const SPIN: f32 = 0.1;
/// Texels a second the near layer's self-lit faces scroll.
const PAN: [f32; 2] = [30.0, 12.0];
/// The near layer's fog.
const FOG: (f32, f32, u32) = (8.0, 60.0, 0x30_4060);
/// The seconds of game time the vista cases are drawn at.
pub const VISTA_TIME: f32 = 3.0;

fn vd(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(min: Vec3d, max: Vec3d, op: Op, order: u32, material: u16) -> Brush {
    Brush::cuboid(min, max, op, order)
        .expect("a cuboid")
        .with_material(material)
}

/// A room of `material` around `x`, 32 wide and 64 deep.
fn room_at(x: f64, order: u32, material: u16) -> Brush {
    cuboid(
        vd(x - 16.0, -16.0, -16.0),
        vd(x + 16.0, 48.0, 16.0),
        Op::Subtract,
        order,
        material,
    )
}

/// A thin square plate `side` wide at `(x, y)`, its near face mapped once with a
/// texture `texels` wide.
fn plate(x: f64, y: f64, side: f64, order: u32, material: u16, texels: u32) -> Brush {
    let half = side / 2.0;
    let min = vd(x - half, y, -half);
    let mut b = cuboid(min, vd(x + half, y + 1.0, half), Op::Add, order, material);
    let face = b.plane_facing(vd(0.0, -1.0, 0.0)).expect("the near face");
    let mut t = b.texinfos()[face];
    let scale = side as f32 / texels as f32;
    t.u_axis = Vec3::new(1.0, 0.0, 0.0);
    t.v_axis = Vec3::new(0.0, 0.0, -1.0);
    t.rotation_deg = 0.0;
    t.set_scale(scale, scale);
    t.u_shift = -(x - half) as f32 / scale;
    t.v_shift = half as f32 / scale;
    assert!(b.set_texinfo(face, t));
    b
}

fn lamp(pos: Vec3, radius: f32) -> Entity {
    Entity::Light {
        pos,
        color: Vec3::new(1.0, 0.95, 0.85),
        radius,
        intensity: 2.0,
    }
}

/// A layer of slot 0 at `x`, seen as from the origin of the play room.
fn layer(x: f64, order: u8, parallax: f32, spin: Vec3, pan: [f32; 2]) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(x as f32, 0.0, 0.0),
        centre: Vec3::ZERO,
        slot: 0,
        layer: order,
        parallax,
        spin,
        pan,
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0,
    }
}

/// A ball of rock in the near layer's room, in front of the planet, carved
/// into a volume of quarter cells 12 units a side.
fn rock_ball(brushes: &[Brush]) -> Volume {
    let origin = [(NEAR_AT as i32 - 12) * 4, 40, -36];
    let mut volume =
        Volume::new(Op::Add, 7, CellSize::Quarter, origin, [3, 3, 3]).expect("a volume");
    let shape = volume.clone();
    let mut ctx = Context::new(&shape, brushes);
    let ball = Prim::sphere(vd(NEAR_AT - 6.0, 16.0, -3.0), 2.5).expect("a ball");
    let rock = DabOp::Add {
        material: ROCK as u8,
    };
    apply_dab(&mut volume, &mut ctx, &ball, rock);
    volume
}

/// The brick room, its window, and the two layers' rooms. The near room's
/// walls are viewport faces, which a layer sees through to the one behind.
pub fn level() -> Level {
    let brushes = vec![
        cuboid(
            vd(-16.0, -16.0, -8.0),
            vd(16.0, 32.0, 8.0),
            Op::Subtract,
            0,
            BRICK,
        ),
        cuboid(
            vd(-8.0, 32.0, -5.0),
            vd(8.0, 33.0, 5.0),
            Op::Subtract,
            1,
            VIEW_FIRST,
        ),
        // Wide, so the window sees only its far wall: a steep wall is where the
        // spans of the two renderers part furthest.
        cuboid(
            vd(STARS_AT - 200.0, -16.0, -200.0),
            vd(STARS_AT + 200.0, 10.0, 200.0),
            Op::Subtract,
            2,
            SKY_STARS,
        ),
        room_at(NEAR_AT, 3, VIEW_FIRST),
        cuboid(
            vd(NEAR_AT - 12.0, 30.0, -12.0),
            vd(NEAR_AT + 2.0, 33.0, -4.0),
            Op::Add,
            4,
            METAL,
        ),
        plate(NEAR_AT - 3.0, 26.0, 12.0, 5, SKY_PLANET, 256),
        plate(NEAR_AT + 4.0, 18.0, 10.0, 6, SKY_CLOUDS, 512),
    ];
    let spin = Vec3::new(0.0, 0.0, SPIN);
    let (fog_start, fog_end, fog_color) = FOG;
    let mut near = layer(NEAR_AT, 1, NEAR_PARALLAX, Vec3::ZERO, PAN);
    if let Entity::VistaLayer {
        fog_start: s,
        fog_end: e,
        fog_color: c,
        ..
    } = &mut near
    {
        (*s, *e, *c) = (fog_start, fog_end, fog_color);
    }
    Level {
        volumes: vec![rock_ball(&brushes)],
        brushes,
        entities: vec![
            lamp(Vec3::new(0.0, 4.0, 5.0), 40.0),
            lamp(Vec3::new(NEAR_AT as f32 - 4.0, 12.0, 10.0), 50.0),
            layer(STARS_AT, 0, 0.0, spin, [0.0; 2]),
            near,
        ],
        ..Level::default()
    }
}

/// `level` with `without` taken out of its vista, when that is what it takes.
pub fn without(mut level: Level, without: Option<Taken>) -> Level {
    for entity in &mut level.entities {
        if let Entity::VistaLayer {
            parallax,
            spin,
            pan,
            fog_end,
            ..
        } = entity
        {
            match without {
                Some(Taken::Parallax) => *parallax = 0.0,
                Some(Taken::Spin) => *spin = Vec3::ZERO,
                Some(Taken::Pan) => *pan = [0.0; 2],
                Some(Taken::VistaFog) => *fog_end = MAX_VISTA_FOG,
                _ => {}
            }
        }
    }
    if matches!(without, Some(Taken::Vistas)) {
        level
            .entities
            .retain(|e| !matches!(e, Entity::VistaLayer { .. }));
    }
    level
}

/// The stock materials with the cutouts' coverage taken away: each the same
/// colours, all solid.
pub fn solid_cutouts() -> MaterialTable {
    let mut table = real_materials();
    for id in [SKY_PLANET, SKY_CLOUDS] {
        let material = table.get(MaterialId(id)).cloned().expect("a stock cutout");
        let t = &material.texture;
        let texels = (0..t.height() as i32)
            .flat_map(|y| (0..t.width() as i32).map(move |x| (x, y)))
            .map(|(x, y)| t.texel(x, y) | 0xff00_0000)
            .collect();
        let texture = Texture::new(t.width(), t.height(), texels).expect("same size");
        let mut solid = material;
        solid.texture = texture;
        table.install(solid).expect("room in the table");
    }
    table
}

impl Scene {
    /// [`level`] with fine enough luxels to light the block.
    pub fn vista() -> Self {
        Self {
            level: level(),
            spotless: None,
            cfg: BuildConfig {
                luxel_size: 0.5,
                shadow_samples: 4,
                ..BuildConfig::default()
            },
            bakes: RefCell::new(BakeCache::new()),
            materials: real_materials,
        }
    }
}
