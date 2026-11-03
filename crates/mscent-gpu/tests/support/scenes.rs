//! What the parity cases draw: levels, where to stand in them, and each way of
//! taking a feature out of a software frame. Every removal is ordinary input, so
//! no test hook ships in a renderer.

#[path = "vista_scene.rs"]
mod vista_scene;

use std::cell::RefCell;
use std::path::Path;

use mscent_math::{Quat, Vec3, Vec3d};
use mscent_render::{
    Actor, BakeCache, BuildConfig, DynLight, LevelRenderer, Material, MaterialTable, MoverPose,
    Quality, Texture, View,
};
use mscent_world::materials::{BRICK, CAP, METAL, ROCK, WAX};
use mscent_world::voxel::{CellSize, Context, DabOp, Prim, Volume, apply_dab};
use mscent_world::{Brush, Entity, Level, MoverGroup, Op, materials, showcase_level};

/// How far from a surface `Pose::AtMaterial` stands.
const NEAR_A_SURFACE: f32 = 3.0;
/// How far `Pose::FarFromMaterial` stands, where mips and bump fades show.
const FAR_FROM_A_SURFACE: f32 = 22.0;
/// A case's dynamic light: how far it reaches, and its colour, warm and past
/// full so that it shows over the baked light.
const LIGHT_RADIUS: f32 = 10.0;
const LIGHT_COLOR: Vec3 = Vec3::new(1.6, 1.0, 0.5);

/// What a case takes out of its software frame, to prove it can fail.
#[derive(Clone, Copy)]
pub enum Taken {
    SkyColors,
    Fog,
    Textures,
    Lamps,
    Quality(Quality),
    Actors,
    MoverPose,
    /// The level's voxel volumes.
    Voxels,
    /// The spotted material, painted as rock instead.
    Spots,
    /// The frame's dynamic lights.
    DynLights,
    /// The level's vista layers, so its viewports show the gradient.
    Vistas,
    /// The layers' parallax, so their eyes stay put.
    Parallax,
    /// The layers' spin.
    Spin,
    /// The layers' pan.
    Pan,
    /// The near layer's fog.
    VistaFog,
    /// The cutouts' coverage, so they draw solid.
    Cutouts,
}

/// Where a case stands.
#[derive(Clone, Copy)]
pub enum Pose {
    OutsideTheLevel,
    DownTheHall,
    /// Down at the floor around the pillar, where a hard shadow falls on a surface
    /// that is not steep.
    AtTheFloor,
    /// Facing the largest face of a material.
    AtMaterial(&'static str),
    FarFromMaterial(&'static str),
    AtActor,
    AtDoor,
    /// At the door swung open about its hinge, so a mover's pose turns it.
    AtTurnedDoor,
    /// At the window of [`Scene::vista`], square on.
    AtVista,
    /// At the same window from aside, where parallax moves the near layer. Still
    /// square on: the stars are where a slant parts the two renderers' spans.
    AtVistaAside,
    /// From `eye`, looking at `at`.
    Looking {
        eye: Vec3,
        at: Vec3,
    },
}

/// A frame's inputs, as a case draws them.
pub struct Framing {
    pub view: View,
    pub actors: Vec<Actor>,
    pub poses: Vec<MoverPose>,
    pub lights: Vec<DynLight>,
    /// Seconds of game time, for a vista's spin and pan.
    pub time: f32,
}

/// A level, and its bake shared by all the frames of it.
pub struct Scene {
    level: Level,
    /// The same level with no spots, for `Taken::Spots`.
    spotless: Option<Level>,
    cfg: BuildConfig,
    bakes: RefCell<BakeCache>,
    materials: fn() -> MaterialTable,
}

fn real_materials() -> MaterialTable {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    MaterialTable::load_dir(&dir).expect("textures")
}

/// Every material one colour. Not a grey: a grey would not see channels swapped.
const FLAT_COLOR: u32 = 0xff40_80c0;

fn flat_materials() -> MaterialTable {
    let mut table = MaterialTable::new();
    for name in materials::NAMES {
        table.add(Material {
            name: name.to_string(),
            gloss: 0,
            texture: Texture::solid(FLAT_COLOR),
        });
    }
    table
}

fn looking(eye: Vec3, along: Vec3) -> View {
    let yaw = (-along.x).atan2(along.y);
    let pitch = along.z.clamp(-1.0, 1.0).asin();
    View::from_angles(eye, yaw, pitch, 0.0)
}

impl Scene {
    pub fn showcase() -> Self {
        Self {
            level: showcase_level(),
            spotless: None,
            cfg: BuildConfig::default(),
            bakes: RefCell::new(BakeCache::new()),
            materials: real_materials,
        }
    }

    /// The showcase with hard shadows, fine luxels, one colour on every wall and no
    /// fog, so all that shows is which light level a pixel took. Fine luxels make a
    /// pixel cover several from an ordinary distance, soft light would hide a wrong
    /// level, and so would fog.
    pub fn hard_shadows() -> Self {
        Self {
            level: showcase_level(),
            spotless: None,
            cfg: BuildConfig {
                luxel_size: 0.03125,
                light_radius: 0.0,
                shadow_samples: 1,
                fog_start: 1.0e5,
                fog_end: 2.0e5,
                ..BuildConfig::default()
            },
            bakes: RefCell::new(BakeCache::new()),
            materials: flat_materials,
        }
    }

    /// A wide room with a door across it at y = 8, lit on the near side. Wide, as
    /// a steep brick wall is where the spans of the two renderers part furthest,
    /// past the pixel the comparison allows, and this case is about the door.
    pub fn door() -> Self {
        let corner = |x, y, z| Vec3d::new(x, y, z);
        let room = Brush::cuboid(
            corner(-40.0, -8.0, -8.0),
            corner(40.0, 24.0, 8.0),
            Op::Subtract,
            0,
        )
        .expect("a room");
        let door = Brush::cuboid(corner(-3.0, 8.0, -3.0), corner(3.0, 9.0, 3.0), Op::Add, 1)
            .expect("a door");
        let level = Level {
            brushes: vec![room, door.with_material(METAL)],
            entities: vec![Entity::Light {
                pos: Vec3::new(0.0, 2.0, 3.0),
                color: Vec3::new(1.0, 1.0, 1.0),
                radius: 30.0,
                intensity: 3.0,
            }],
            mover_groups: vec![MoverGroup {
                name: "door".to_string(),
                tags: Vec::new(),
                brushes: vec![1],
            }],
            ..Level::default()
        };
        let cfg = BuildConfig {
            luxel_size: 0.5,
            shadow_samples: 4,
            ..BuildConfig::default()
        };
        Self {
            level,
            spotless: None,
            cfg,
            bakes: RefCell::new(BakeCache::new()),
            materials: real_materials,
        }
    }

    /// A long room with voxel balls on its floor: rock sunk into the floor, wax
    /// against the rock, so two materials meet, a spotted cap, and a pebble
    /// between the lamp and the rock, so a shadow falls on a voxel surface. A
    /// brick block stands to one side.
    pub fn voxel_room() -> Self {
        Self::voxels_in_fog(
            BuildConfig::default().fog_start,
            BuildConfig::default().fog_end,
        )
    }

    /// The voxel room with its fog far off, so that distant voxels show their
    /// mips and detail levels. Not for near cases: it shows the far brick walls
    /// too, where faces part by more than voxels do.
    pub fn voxel_distance() -> Self {
        Self::voxels_in_fog(20.0, 90.0)
    }

    fn voxels_in_fog(fog_start: f32, fog_end: f32) -> Self {
        Self {
            level: voxel_room(CAP as u8),
            spotless: Some(voxel_room(ROCK as u8)),
            cfg: BuildConfig {
                luxel_size: 0.5,
                shadow_samples: 4,
                fog_start,
                fog_end,
                ..BuildConfig::default()
            },
            bakes: RefCell::new(BakeCache::new()),
            materials: real_materials,
        }
    }

    /// The level's software renderer, with `without` taken out of it. A removal
    /// that a frame's inputs make is left for the caller.
    pub fn software(&self, without: Option<Taken>) -> LevelRenderer {
        let mut cfg = self.cfg;
        let mut level = self.level.clone();
        let mut materials = self.materials;
        match without {
            Some(Taken::SkyColors) => (cfg.sky_top, cfg.sky_bottom) = (cfg.sky_bottom, cfg.sky_top),
            Some(Taken::Fog) => (cfg.fog_start, cfg.fog_end) = (1.0e5, 2.0e5),
            Some(Taken::Textures) => materials = flat_materials,
            Some(Taken::Lamps) => level
                .entities
                .retain(|e| !matches!(e, Entity::Light { .. })),
            Some(Taken::Voxels) => level.volumes.clear(),
            Some(Taken::Spots) => level = self.spotless.clone().expect("a spotted level"),
            Some(Taken::Cutouts) => materials = vista_scene::solid_cutouts,
            _ => {}
        }
        let level = vista_scene::without(level, without);
        let mut bakes = self.bakes.borrow_mut();
        LevelRenderer::from_level_cached(&level, materials(), &cfg, &mut bakes)
            .expect("the level builds")
    }

    fn at_material(&self, name: &str, distance: f32) -> View {
        let id = materials::NAMES
            .iter()
            .position(|&n| n == name)
            .expect("a stock material") as u16;
        let renderer = self.software(None);
        let scene = renderer.scene();
        let area = |poly: &[mscent_render::Vertex]| {
            let bounds = poly
                .iter()
                .fold(mscent_math::Aabb::empty(), |b, v| b.including(v.pos));
            (bounds.max - bounds.min).length()
        };
        let (face, poly) = scene
            .faces()
            .iter()
            .zip(scene.polys())
            .filter(|(face, _)| face.material == id)
            .max_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .expect("a face of that material");
        let centre = poly.iter().fold(Vec3::ZERO, |sum, v| sum + v.pos) * (1.0 / poly.len() as f32);
        let n = face.plane.normal;
        let normal = Vec3::new(n.x as f32, n.y as f32, n.z as f32);
        // A little off square, so the specular lobe and the mips have a slope.
        let eye = centre + normal * distance + Vec3::new(0.7, 0.4, 0.3);
        let along = (centre - eye).normalized().expect("not at the face");
        looking(eye, along)
    }

    pub fn framing(&self, pose: Pose) -> Framing {
        let renderer = self.software(None);
        let view = match pose {
            Pose::OutsideTheLevel => {
                looking(Vec3::new(-500.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 0.0))
            }
            Pose::DownTheHall => View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0),
            Pose::AtTheFloor => View::from_angles(Vec3::new(3.0, 3.0, 5.0), -0.78, -0.75, 0.0),
            Pose::AtMaterial(name) => self.at_material(name, NEAR_A_SURFACE),
            Pose::FarFromMaterial(name) => self.at_material(name, FAR_FROM_A_SURFACE),
            Pose::AtActor => {
                let actor = renderer.actors().first().expect("an actor");
                looking(
                    actor.pos - Vec3::new(0.0, 4.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                )
            }
            Pose::AtDoor | Pose::AtTurnedDoor => looking(Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0)),
            Pose::AtVista => looking(Vec3::new(0.0, 16.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            Pose::AtVistaAside => looking(Vec3::new(3.0, 14.0, 1.0), Vec3::new(0.0, 1.0, 0.0)),
            Pose::Looking { eye, at } => looking(eye, (at - eye).normalized().expect("apart")),
        };
        let time = match pose {
            Pose::AtVista | Pose::AtVistaAside => vista_scene::VISTA_TIME,
            _ => 0.0,
        };
        let poses = match pose {
            Pose::AtDoor => vec![MoverPose {
                offset: Vec3::new(2.0, 0.0, 0.0),
                ..MoverPose::AUTHORED
            }],
            Pose::AtTurnedDoor => vec![MoverPose {
                rot: Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.35),
                pivot: Vec3::new(-3.0, 8.0, 0.0),
                offset: Vec3::ZERO,
            }],
            _ => Vec::new(),
        };
        Framing {
            view,
            actors: renderer.actors().to_vec(),
            poses,
            lights: Vec::new(),
            time,
        }
    }

    /// Adds to `framing` a light `ahead` units in front of its eye, a little
    /// to the right and up, so that it falls on what the case looks at.
    pub fn light(&self, framing: &mut Framing, ahead: f32) {
        let renderer = self.software(None);
        let view = &framing.view;
        let pos = view.pos + view.forward() * ahead + Vec3::new(0.4, 0.0, 0.3);
        let mut light = DynLight::new();
        let placed = light.place(renderer.scene(), pos, LIGHT_RADIUS, LIGHT_COLOR);
        assert!(placed, "a light at {pos:?}");
        framing.lights.push(light);
    }
}

/// The level of [`Scene::voxel_room`], with its cap of `cap`.
fn voxel_room(cap: u8) -> Level {
    let corner = |x, y, z| Vec3d::new(x, y, z);
    let room = Brush::cuboid(
        corner(-20.0, -40.0, -4.0),
        corner(20.0, 14.0, 8.0),
        Op::Subtract,
        0,
    )
    .expect("a room");
    // Quarter cells, four units to a chunk: x -8..8, y 0..12, z -4..4.
    let mut volume =
        Volume::new(Op::Add, 1, CellSize::Quarter, [-32, 0, -16], [4, 3, 2]).expect("a volume");
    let shape = volume.clone();
    let brushes = vec![room];
    let mut ctx = Context::new(&shape, &brushes);
    let balls = [
        (corner(0.0, 5.0, -2.5), 3.0, ROCK as u8),
        (corner(3.6, 7.5, -2.0), 2.0, WAX as u8),
        (corner(-4.5, 6.0, -2.0), 2.0, cap),
        (corner(0.0, 2.2, 2.4), 0.7, ROCK as u8),
    ];
    for (centre, radius, material) in balls {
        let ball = Prim::sphere(centre, radius).expect("a ball");
        apply_dab(&mut volume, &mut ctx, &ball, DabOp::Add { material });
    }
    // Flat and in brick, whose mortar shows which mip level a far wall took.
    let block = Prim::cuboid(corner(5.5, 1.0, -4.0), corner(7.5, 3.0, -1.5)).expect("a block");
    let brick = DabOp::Add {
        material: BRICK as u8,
    };
    apply_dab(&mut volume, &mut ctx, &block, brick);
    Level {
        brushes,
        volumes: vec![volume],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 0.5, 5.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 30.0,
            intensity: 2.0,
        }],
        ..Level::default()
    }
}
