//! What the parallax tests draw and what they know by hand: a room whose floor is
//! one material with a height map and an albedo that names its own texels, so that a
//! pixel's colour says which texel it shows, and the geometry to say which texel it
//! should show.

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Camera, Effects, FrameInput, Framebuffer, LevelRenderer, Material, MaterialTable,
    Quality, Texture, View,
};
use mscent_world::{Brush, Entity, Level, Op};

/// Side of the material's texture, in texels.
pub const SIDE: u32 = 64;
/// How many levels of a channel a texel step is: the coded albedo's slope.
pub const CODE: f32 = 3.0;
/// Where the room's floor is.
pub const FLOOR_Z: f32 = -4.0;

/// The height of a texel, 0 to 255.
pub type Heights = Box<dyn Fn(u32, u32) -> u8>;

/// An albedo whose red is three times its column and green three times its row, so
/// that colour says texel. Blue is constant: the light's reading of a flat grey.
fn coded(grey: bool) -> Vec<u32> {
    let mut texels = Vec::new();
    for y in 0..SIDE {
        for x in 0..SIDE {
            let (r, g) = if grey { (128, 128) } else { (x * 3, y * 3) };
            texels.push(0xff00_0000 | r << 16 | g << 8 | 128);
        }
    }
    texels
}

/// A material with `heights` and a relief `depth` texels deep, and the albedo
/// that names texels, or one grey for reading the light off.
pub fn table(heights: &Heights, depth: u8, grey: bool) -> MaterialTable {
    let map: Vec<u8> = (0..SIDE * SIDE)
        .map(|i| heights(i % SIDE, i / SIDE))
        .collect();
    let texture = Texture::new(SIDE, SIDE, coded(grey))
        .and_then(|t| t.with_heights(map))
        .expect("a texture")
        .with_relief(depth)
        .filtered();
    // A level needs every stock id; the room uses the first.
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

/// A room, lit from above and ahead, whose every wall is material 0.
pub fn room(materials: MaterialTable) -> LevelRenderer {
    let corner = |x, y, z| Vec3d::new(x, y, z);
    let brush = Brush::cuboid(
        corner(-12.0, -12.0, f64::from(FLOOR_Z)),
        corner(12.0, 40.0, 8.0),
        Op::Subtract,
        0,
    )
    .expect("a room");
    let level = Level {
        brushes: vec![brush],
        entities: vec![Entity::Light {
            pos: Vec3::new(0.0, 10.0, 6.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 60.0,
            intensity: 3.0,
        }],
        ..Level::default()
    };
    let cfg = BuildConfig {
        luxel_size: 0.5,
        shadow_samples: 1,
        light_radius: 0.0,
        fog_start: 1.0e5,
        fog_end: 2.0e5,
        ..BuildConfig::default()
    };
    LevelRenderer::from_level(&level, materials, &cfg).expect("the room builds")
}

/// One frame of `level` from `view` at `size`, with the GPU-only looks as given.
pub fn draw(
    gpu: &Gpu,
    level: &LevelRenderer,
    view: &View,
    size: (u32, u32),
    effects: GpuEffects,
) -> Vec<u32> {
    let mut renderer =
        GpuRenderer::with_finish(gpu.clone(), level.assets(), Finish::Reference).expect("it fits");
    renderer.set_effects(Some(effects));
    let mut fb = Framebuffer::new(size.0, size.1).expect("a framebuffer");
    let input = FrameInput {
        view,
        quality: Quality::High,
        actors: &[],
        effects: Effects::default(),
        wants_depth: false,
    };
    renderer
        .draw(level.assets(), &mut fb, &input)
        .expect("a frame");
    assert_eq!(renderer.take_error(), None);
    fb.pixels().to_vec()
}

/// From `eye`, looking down the room, tipped down by `pitch`.
pub fn looking(eye: Vec3, pitch: f32) -> View {
    View::from_angles(eye, 0.0, pitch, 0.0)
}

/// What a camera and the floor say about where a pixel lands, by hand.
pub struct Floor {
    pub camera: Camera,
    pub eye: Vec3,
    /// Texel coordinates of a floor point `(x, y)`: `u = m[0] x + m[1] y + m[2]`,
    /// `v = m[3] x + m[4] y + m[5]`.
    pub texel_of: [f32; 6],
    /// World size of a texel along the floor, where its map is square.
    pub world_per_texel: f32,
}

impl Floor {
    pub fn new(level: &LevelRenderer, view: &View, size: (u32, u32)) -> Self {
        let camera = level.camera(view, size.0, size.1).expect("a camera");
        let scene = level.scene();
        let (face, poly) = scene
            .faces()
            .iter()
            .zip(scene.polys())
            .find(|(f, _)| f.plane.normal.z > 0.99)
            .expect("a floor");
        assert!((poly[0].pos.z - FLOOR_Z).abs() < 1e-3, "{face:?}");
        let texels = |i: usize| (poly[i].uv.x * SIDE as f32, poly[i].uv.y * SIDE as f32);
        let p = |i: usize| (poly[i].pos.x, poly[i].pos.y);
        // The map through the first corner and the two that make the widest triangle.
        let n = poly.len();
        let (a, b, c) = (0, n / 3, 2 * n / 3);
        let (ax, ay, bx, by, cx, cy) = (p(a).0, p(a).1, p(b).0, p(b).1, p(c).0, p(c).1);
        let det = (bx - ax) * (cy - ay) - (cx - ax) * (by - ay);
        assert!(det.abs() > 1e-3, "corners in a line");
        let solve = |value: [f32; 3]| {
            let (db, dc) = (value[1] - value[0], value[2] - value[0]);
            let gx = (db * (cy - ay) - dc * (by - ay)) / det;
            let gy = (dc * (bx - ax) - db * (cx - ax)) / det;
            [gx, gy, value[0] - gx * ax - gy * ay]
        };
        let u = solve([texels(a).0, texels(b).0, texels(c).0]);
        let v = solve([texels(a).1, texels(b).1, texels(c).1]);
        let world_per_texel = 1.0 / (u[0] * v[1] - u[1] * v[0]).abs().sqrt();
        Self {
            camera,
            eye: view.pos,
            texel_of: [u[0], u[1], u[2], v[0], v[1], v[2]],
            world_per_texel,
        }
    }

    /// Texels the floor covers per pixel at screen point `(sx, sy)`: the larger of
    /// its steps along the two screen axes, as the shader measures it.
    pub fn texel_rate(&self, sx: f32, sy: f32) -> Option<f32> {
        let here = self.point_under(sx, sy)?;
        let (u, v) = self.texel_at(here.x, here.y);
        let step = |dx: f32, dy: f32| {
            let p = self.point_under(sx + dx, sy + dy)?;
            let (pu, pv) = self.texel_at(p.x, p.y);
            Some((pu - u).abs().max((pv - v).abs()))
        };
        Some(step(1.0, 0.0)?.max(step(0.0, 1.0)?))
    }

    pub fn texel_at(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.texel_of;
        (m[0] * x + m[1] * y + m[2], m[3] * x + m[4] * y + m[5])
    }

    /// The world point of the floor under screen point `(sx, sy)`, if that is the
    /// floor and not a wall.
    pub fn point_under(&self, sx: f32, sy: f32) -> Option<Vec3> {
        let cam = &self.camera;
        let (w, h, f) = (cam.width() as f32, cam.height() as f32, cam.focal());
        let ray = Vec3::new((sx - w * 0.5) / f, -(sy - h * 0.5) / f, -1.0);
        // R takes world to camera, so R transposed takes the ray back: world axis i
        // of it is the ray dotted with the camera's image of that axis.
        let origin = cam.world_to_camera(self.eye);
        let image = |along: Vec3| cam.world_to_camera(self.eye + along) - origin;
        let dot = |a: Vec3| a.x * ray.x + a.y * ray.y + a.z * ray.z;
        let world = Vec3::new(
            dot(image(Vec3::new(1.0, 0.0, 0.0))),
            dot(image(Vec3::new(0.0, 1.0, 0.0))),
            dot(image(Vec3::new(0.0, 0.0, 1.0))),
        );
        if world.z >= -1e-4 {
            return None;
        }
        let t = (FLOOR_Z - self.eye.z) / world.z;
        let p = self.eye + world * t;
        // In a convex room, the floor plane's points inside its walls are the floor.
        let inside = p.x.abs() < 11.5 && (-11.5..39.5).contains(&p.y);
        inside.then_some(p)
    }
}

/// The height of the map at continuous texel coordinates, 0 to 1 with its highest
/// texel counted as 1, bilinear and wrapping as the sampler is.
fn height_at(heights: &Heights, top: f32, (u, v): (f32, f32)) -> f32 {
    let side = SIDE as i32;
    let (x, y) = (u - 0.5, v - 0.5);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |dx: i32, dy: i32| {
        let wrap = |t: f32, d: i32| (t as i32 + d).rem_euclid(side) as u32;
        f32::from(heights(wrap(x0, dx), wrap(y0, dy))) / top
    };
    let near = at(0, 0) * (1.0 - fx) + at(1, 0) * fx;
    let far = at(0, 1) * (1.0 - fx) + at(1, 1) * fx;
    near * (1.0 - fy) + far * fy
}

/// The highest texel of a map, which a march counts as the face's plane.
pub fn top_of(heights: &Heights) -> f32 {
    let all = (0..SIDE * SIDE).map(|i| heights(i % SIDE, i / SIDE));
    f32::from(all.max().expect("texels").max(1))
}

impl Floor {
    /// The texel an eye at `self.eye` sees at floor point `p`, by marching the ray
    /// into the relief in `steps` equal parts and taking the first below the
    /// surface. Every part is `1 / steps` of the relief's depth, so the answer
    /// is within a part of the truth, which a million steps make nothing.
    pub fn marched(&self, heights: &Heights, depth: u8, p: Vec3, steps: u32) -> (f32, f32) {
        let top = top_of(heights);
        let to_eye = self.eye - p;
        let slope = (-to_eye.x / to_eye.z, -to_eye.y / to_eye.z);
        let reach = f32::from(depth) * self.world_per_texel;
        let m = &self.texel_of;
        let start = self.texel_at(p.x, p.y);
        let at = |f: f32| {
            let (sx, sy) = (slope.0 * reach * f, slope.1 * reach * f);
            (
                start.0 + m[0] * sx + m[1] * sy,
                start.1 + m[3] * sx + m[4] * sy,
            )
        };
        for i in 1..=steps {
            let f = i as f32 / steps as f32;
            if height_at(heights, top, at(f)) >= 1.0 - f {
                return at(f);
            }
        }
        at(1.0)
    }
}

/// Channel `shift` bits up of a pixel, in levels.
pub fn channel(pixel: u32, shift: u32) -> f32 {
    ((pixel >> shift) & 0xff) as f32
}

/// The texel a pixel of the coded frame shows, read from its colour against the
/// same pixel of the grey frame, which holds what the light made of 128.
pub fn decode(coded: u32, grey: u32) -> Option<(f32, f32)> {
    let (gr, gg) = (channel(grey, 16), channel(grey, 8));
    // Too dark to read finely, or so bright that the light clamped a channel.
    let clamped = channel(coded, 16) > 253.0 || channel(coded, 8) > 253.0;
    if gr < 48.0 || gg < 48.0 || clamped {
        return None;
    }
    let read = |c: f32, g: f32| (c / g * 128.0) / CODE + 0.5;
    Some((read(channel(coded, 16), gr), read(channel(coded, 8), gg)))
}

impl Floor {
    /// Where on screen the texel `at` of the floor's map shows if the surface there
    /// lies `below` world units under the face, of the repeats in view the one
    /// nearest a point low in the screen, where the texture is magnified.
    pub fn screen_of_texel(&self, at: (f32, f32), below: f32) -> Option<(f32, f32)> {
        let m = &self.texel_of;
        let det = m[0] * m[4] - m[1] * m[3];
        let side = SIDE as f32;
        let (w, h) = (self.camera.width() as f32, self.camera.height() as f32);
        let mut best: Option<(f32, (f32, f32))> = None;
        for a in -3..=3 {
            for b in -3..=3 {
                let du = at.0 + side * a as f32 - m[2];
                let dv = at.1 + side * b as f32 - m[5];
                let x = (m[4] * du - m[1] * dv) / det;
                let y = (-m[3] * du + m[0] * dv) / det;
                let seen = self
                    .camera
                    .world_to_camera(Vec3::new(x, y, FLOOR_Z - below));
                let Some(s) = self.camera.project(seen) else {
                    continue;
                };
                let off = (s.x - w * 0.5).hypot(s.y - h * 0.75);
                let in_view = (0.0..w).contains(&s.x) && (0.0..h).contains(&s.y);
                if in_view && best.is_none_or(|(d, _)| off < d) {
                    best = Some((off, (s.x, s.y)));
                }
            }
        }
        best.map(|(_, s)| s)
    }
}
