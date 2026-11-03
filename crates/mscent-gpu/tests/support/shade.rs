//! What the shadow tests draw and know by hand: a flat grey room with one dynamic
//! light and a box of known size hung in its air, so that where the box's shadow
//! falls on the floor is a sum done on paper.

use std::path::Path;
use std::sync::OnceLock;

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::{Quat, Vec3, Vec3d};
use mscent_render::model::{ModelId, ModelInstance, ModelTable};
use mscent_render::{
    Actor, BuildConfig, Camera, DynLight, Effects, FrameInput, Framebuffer, LevelRenderer,
    Material, MaterialTable, MoverPose, Quality, Texture, View,
};
use mscent_world::model::{build, read};
use mscent_world::{Brush, Entity, Level, MoverGroup, Op};

pub const FLOOR_Z: f32 = -4.0;
pub const SIZE: (u32, u32) = (320, 180);
/// The room's half width and length.
pub const HALF: f64 = 12.0;

/// A 2 by 2 by 2 box: `scale` 0.5 and a cube of half 2 model units.
const BOX: &str = "miniscent-blueprint 1
name box
class drone
scale 0.5
skin hull metal

part body cell=0.5
solid cube 0 0 0  2 2 2 hull
";

/// One flat grey for every stock material, so that light is all a pixel shows.
pub fn grey() -> MaterialTable {
    let texture = Texture::new(8, 8, vec![0xff80_8080; 64])
        .expect("a texture")
        .filtered();
    let mut table = MaterialTable::new();
    for name in mscent_world::materials::NAMES {
        table.add(Material {
            name: name.to_string(),
            gloss: 0,
            texture: texture.clone(),
        });
    }
    table
}

fn cuboid(a: [f64; 3], b: [f64; 3], op: Op, order: u32) -> Brush {
    let v = |p: [f64; 3]| Vec3d::new(p[0], p[1], p[2]);
    Brush::cuboid(v(a), v(b), op, order).expect("a brush")
}

/// What the room holds besides its walls.
#[derive(Clone, Copy, Default)]
pub struct Room {
    /// A baked lamp, its place and intensity, whose light a pillar then shadows in the bake.
    pub baked_lamp: Option<(Vec3, f32)>,
    /// A pillar, 2 wide, from the floor to the ceiling at the room's centre.
    pub pillar: bool,
    /// A mover the size of the box, where the box hangs, to be moved by a pose.
    pub slab: bool,
}

pub fn room(spec: Room) -> LevelRenderer {
    let mut brushes = vec![cuboid(
        [-HALF, -HALF, f64::from(FLOOR_Z)],
        [HALF, HALF, 8.0],
        Op::Subtract,
        0,
    )];
    if spec.pillar {
        brushes.push(cuboid(
            [-1.0, -1.0, f64::from(FLOOR_Z)],
            [1.0, 1.0, 8.0],
            Op::Add,
            1,
        ));
    }
    let mut mover_groups = Vec::new();
    if spec.slab {
        brushes.push(cuboid([-1.0, -1.0, -2.0], [1.0, 1.0, 0.0], Op::Add, 2));
        mover_groups.push(MoverGroup {
            name: "slab".to_string(),
            tags: Vec::new(),
            brushes: vec![brushes.len() as u32 - 1],
        });
    }
    let entities = spec
        .baked_lamp
        .map(|(pos, intensity)| Entity::Light {
            pos,
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 40.0,
            intensity,
        })
        .into_iter()
        .collect();
    let level = Level {
        brushes,
        entities,
        mover_groups,
        ..Level::default()
    };
    let cfg = BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 4,
        light_radius: 0.0,
        fog_start: 1.0e5,
        fog_end: 2.0e5,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&level, grey(), &cfg).expect("the room builds")
}

/// The box, built once.
pub fn boxes() -> &'static (ModelTable, ModelId) {
    static TABLE: OnceLock<(ModelTable, ModelId)> = OnceLock::new();
    TABLE.get_or_init(|| {
        let blueprint = read(BOX.as_bytes()).expect("the box reads");
        let model = build(&blueprint).expect("the box builds");
        let mut table = ModelTable::new();
        let id = table.add(&blueprint, &model, &grey());
        (table, id)
    })
}

/// The box with its centre at `pos`, upright.
pub fn box_at(pos: Vec3) -> ModelInstance {
    ModelInstance::new(
        boxes().1,
        pos,
        Quat::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.0),
    )
}

/// A light at `pos` that reaches 16 units.
pub fn lamp(level: &LevelRenderer, pos: Vec3) -> DynLight {
    let mut light = DynLight::new();
    assert!(light.place(level.scene(), pos, 16.0, Vec3::new(1.5, 1.5, 1.5)));
    light
}

/// From the room's south wall, looking down at its centre.
pub fn view() -> View {
    View::from_angles(Vec3::new(0.0, -10.5, 2.5), 0.0, -0.5, 0.0)
}

/// What one frame holds beside the room.
#[derive(Default)]
pub struct Contents<'a> {
    pub lights: &'a [DynLight],
    pub boxes: &'a [ModelInstance],
    pub actors: &'a [Actor],
    pub poses: &'a [MoverPose],
}

pub struct Shot {
    pub level: LevelRenderer,
    pub renderer: GpuRenderer,
    /// The size of the frames it draws.
    pub size: (u32, u32),
}

impl Shot {
    pub fn new(gpu: &Gpu, level: LevelRenderer) -> Self {
        let renderer = GpuRenderer::with_finish(gpu.clone(), level.assets(), Finish::Reference)
            .expect("it fits");
        Self {
            level,
            renderer,
            size: SIZE,
        }
    }

    /// A High frame from `view` with `effects` and `contents`.
    pub fn frame(&mut self, view: &View, effects: GpuEffects, contents: &Contents) -> Vec<u32> {
        self.renderer.set_effects(Some(effects));
        let mut fb = Framebuffer::new(self.size.0, self.size.1).expect("a framebuffer");
        let (table, _) = boxes();
        let input = FrameInput {
            view,
            quality: Quality::High,
            actors: contents.actors,
            effects: Effects {
                lights: contents.lights,
                poses: contents.poses,
                models: Some((table, contents.boxes)),
                ..Effects::default()
            },
            wants_depth: false,
        };
        self.renderer
            .draw(self.level.assets(), &mut fb, &input)
            .expect("a frame");
        assert_eq!(self.renderer.take_error(), None);
        fb.pixels().to_vec()
    }

    pub fn camera(&self, view: &View) -> Camera {
        self.level
            .camera(view, self.size.0, self.size.1)
            .expect("a camera")
    }
}

pub const ON: GpuEffects = GpuEffects {
    shadows: true,
    parallax: false,
};
pub const OFF: GpuEffects = GpuEffects::OFF;

/// The pixel a world point lands on, if it is in front of the camera.
pub fn pixel_of(cam: &Camera, p: Vec3) -> (usize, usize) {
    let at = cam.project(cam.world_to_camera(p)).expect("in front");
    (at.x.round() as usize, at.y.round() as usize)
}

/// The brightness of the pixel at `at`: the mean of its channels.
pub fn level_at(frame: &[u32], (x, y): (usize, usize)) -> f32 {
    level_in(frame, SIZE.0 as usize, (x, y))
}

/// The same in a frame `width` pixels wide.
pub fn level_in(frame: &[u32], width: usize, (x, y): (usize, usize)) -> f32 {
    let c = frame[y * width + x];
    ((c >> 16 & 255) + (c >> 8 & 255) + (c & 255)) as f32 / 3.0
}

/// A hash of a frame, for knowing it is the same one.
pub fn fnv(frame: &[u32]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |h, p| {
        (h ^ u64::from(*p)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Writes `frame` as a PNG into `$MSCENT_SHOTS`, when that is set, for looking at.
pub fn keep(name: &str, frame: &[u32]) {
    let Some(dir) = std::env::var_os("MSCENT_SHOTS") else {
        return;
    };
    let png = mscent_render::png_encode::encode_png(SIZE.0, SIZE.1, frame).expect("a png");
    std::fs::write(Path::new(&dir).join(name), png).expect("written");
}
