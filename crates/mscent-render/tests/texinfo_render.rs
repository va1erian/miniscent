//! Per-face texture mapping end to end: a gradient texture turns every
//! rendered pixel into the texel coordinates it sampled, so scale, shift,
//! rotation and flips are checked on real pixels; a second test checks the
//! bump-mapping frame follows the texture axes; a third writes PNGs of the
//! showcase level for a human to inspect (`target/shots/texinfo`).

use std::path::{Path, PathBuf};

use mscent_math::{Vec3, Vec3d};
use mscent_render::{
    BuildConfig, Camera, FlatShader, Framebuffer, LevelRenderer, Material, MaterialTable, Quality,
    Texture, UvToLuxel, View, scene::Scene,
};
use mscent_world::{Brush, Entity, Level, Op, Portals, TexInfo, compile, extract_faces};

const W: u32 = 320;
const H: u32 = 240;
const SIDE: f64 = 16.0;
const HEIGHT: f64 = 8.0;

fn v3(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

/// Texel (x, y) is red 2x, green 2y: the pixel colour names the texel.
fn gradient_table() -> MaterialTable {
    let albedo = (0..128u32 * 128)
        .map(|i| 0xff00_0040 | ((i % 128) * 2) << 16 | ((i / 128) * 2) << 8)
        .collect();
    let mut t = MaterialTable::new();
    t.add(Material {
        name: "gradient".into(),
        gloss: 0,
        texture: Texture::new(128, 128, albedo).expect("texture"),
    });
    t
}

/// One room with a different mapping on every surface.
fn mapped_room() -> Level {
    let mut room =
        Brush::cuboid(v3(0.0, 0.0, 0.0), v3(SIDE, SIDE, HEIGHT), Op::Subtract, 0).expect("room");
    let mut edit = |facing: Vec3d, f: &dyn Fn(&mut TexInfo)| {
        // Subtract brush planes face the solid, so look for the opposite.
        let i = room.plane_facing(facing).expect("plane");
        let mut t = room.texinfos()[i];
        f(&mut t);
        assert!(room.set_texinfo(i, t));
    };
    edit(v3(0.0, 0.0, -1.0), &|t| {
        t.rotate(30.0);
        t.set_scale(0.1, 0.05);
    }); // floor
    edit(v3(-1.0, 0.0, 0.0), &|t| t.scale_by(3.0, 3.0)); // east wall
    edit(v3(1.0, 0.0, 0.0), &|t| t.shift_by(30.0, 17.0)); // west wall
    edit(v3(0.0, 1.0, 0.0), &|t| t.rotate(90.0)); // south wall
    edit(v3(0.0, -1.0, 0.0), &|t| t.flip_u()); // north wall
    Level {
        brushes: vec![room],
        entities: Vec::new(),
    }
}

fn scene_of(level: &Level) -> Scene {
    let bsp = compile(&level.brushes).expect("compiles");
    let faces = extract_faces(&bsp, &level.brushes);
    let portals = Portals::build(&bsp);
    Scene::new(bsp, faces, portals, gradient_table())
}

fn camera(eye: Vec3, target: Vec3) -> Camera {
    Camera::look_at(eye, target, Vec3::new(0.0, 0.0, 1.0), 1.2, 0.1, 100.0, W, H).expect("camera")
}

fn png(fb: &Framebuffer) -> Vec<u8> {
    mscent_render::png_encode::encode_png(fb.width(), fb.height(), fb.pixels()).expect("png")
}

fn shots_dir() -> PathBuf {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots/texinfo");
    std::fs::create_dir_all(&dir).expect("shots dir");
    dir
}

/// True if `got` (a colour channel) is within a few texels of `texel`'s
/// channel value, wrapping at 128 texels.
fn channel_matches(got: u32, texel: f64) -> bool {
    let wrapped = texel.rem_euclid(128.0);
    // Stay clear of the seam where one pixel can fall on either side.
    if !(4.0..124.0).contains(&wrapped) {
        return true;
    }
    (f64::from(got) - wrapped * 2.0).abs() <= 14.0
}

#[test]
fn rendered_pixels_follow_each_faces_mapping() {
    let level = mapped_room();
    let scene = scene_of(&level);
    let infos: Vec<TexInfo> = scene.faces().iter().map(|f| f.texinfo).collect();
    assert!(infos.iter().any(|t| t.rotation_deg != 0.0));
    // Look at each surface from inside the room, with sample points on it.
    let c = SIDE as f32 / 2.0;
    let mid = HEIGHT as f32 / 2.0;
    let views = [
        // floor, looking down from the south west corner
        (Vec3::new(2.0, 2.0, 6.0), Vec3::new(c, c, 0.0), [0.5, 0.0]),
        // east wall (x = 0 is the "west" in the plan, x = 16 the east)
        (
            Vec3::new(c, c, mid),
            Vec3::new(SIDE as f32, c, mid),
            [1.0, 0.0],
        ),
        (Vec3::new(c, c, mid), Vec3::new(0.0, c, mid), [-1.0, 0.0]),
        (
            Vec3::new(c, c, mid),
            Vec3::new(c, SIDE as f32, mid),
            [0.0, 1.0],
        ),
        (Vec3::new(c, c, mid), Vec3::new(c, 0.0, mid), [0.0, -1.0]),
    ];
    let mut checked_total = 0;
    for (n, (eye, target, _)) in views.iter().enumerate() {
        let cam = camera(*eye, *target);
        let mut fb = Framebuffer::new(W, H).expect("fb");
        scene.render_with(&mut fb, &cam, None, &mut |_, _| FlatShader::new(256, None));
        std::fs::write(shots_dir().join(format!("gradient_{n}.png")), png(&fb)).expect("write");
        let mut checked = 0;
        for face in scene.faces() {
            // Sample points on the face that the camera sees.
            let verts: Vec<Vec3d> = face.polygon.vertices().to_vec();
            let centre = verts.iter().fold(Vec3d::ZERO, |a, &p| a + p) * (1.0 / verts.len() as f64);
            if face.plane.signed_distance(eye.to_f64()) <= 0.0 {
                continue;
            }
            for (a, b) in [
                (0.0, 0.0),
                (0.3, 0.1),
                (-0.3, 0.2),
                (0.1, -0.35),
                (-0.2, -0.25),
            ] {
                let (e1, e2) = (verts[1] - verts[0], verts[verts.len() - 1] - verts[0]);
                let p = centre + e1 * a + e2 * b;
                let Some(s) = cam.project(cam.world_to_camera(p.to_f32())) else {
                    continue;
                };
                let (px, py) = (s.x as i32, s.y as i32);
                if px < 2 || py < 2 || px >= W as i32 - 2 || py >= H as i32 - 2 {
                    continue;
                }
                let pix = fb.pixels()[py as usize * W as usize + px as usize];
                let (tu, tv) = face.texinfo.texels_at(p);
                let (r, g) = ((pix >> 16) & 0xff, (pix >> 8) & 0xff);
                assert!(
                    channel_matches(r, tu) && channel_matches(g, tv),
                    "view {n} at {p:?}: pixel ({r}, {g}) vs texel ({tu:.1}, {tv:.1}) with {:?}",
                    face.texinfo
                );
                checked += 1;
            }
        }
        assert!(checked >= 3, "view {n} only checked {checked} points");
        checked_total += checked;
    }
    assert!(checked_total >= 20, "{checked_total}");
}

/// A room with a lamp, real textures, for lighting checks.
fn lit_room(edit: impl Fn(&mut Brush)) -> Level {
    let mut room =
        Brush::cuboid(v3(0.0, 0.0, 0.0), v3(SIDE, SIDE, HEIGHT), Op::Subtract, 0).expect("room");
    edit(&mut room);
    Level {
        brushes: vec![room],
        entities: vec![Entity::Light {
            pos: Vec3::new(8.0, 8.0, 6.0),
            color: Vec3::new(1.0, 1.0, 1.0),
            radius: 30.0,
            intensity: 1.0,
        }],
    }
}

fn real_renderer(level: &Level) -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(level, materials, &BuildConfig::default()).expect("builds")
}

/// For the face with visible `normal`, checks the bump frame: the world
/// direction in which the image's right edge lies (+u, or -u when mirrored)
/// maps to texture-frame +x and the direction of image-up maps to +y.
fn assert_bump_frame_follows_axes(r: &LevelRenderer, normal: Vec3d) {
    let index = r
        .scene()
        .faces()
        .iter()
        .position(|f| (f.plane.normal - normal).length() < 1e-9)
        .expect("face");
    let face = &r.scene().faces()[index];
    let proj = &r.lightmap().projections()[index];
    let tex = r.scene().texture_of(face.material);
    let map = UvToLuxel::from_vertices(proj, &r.scene().polys()[index], tex).expect("map");
    let (u, v) = face.texinfo.effective_axes();
    // Express a world direction in the lightmap tangent frame, then back
    // through the shader's conversion into the texture frame.
    let to_tex = |d: Vec3d| {
        let d = d.to_f32();
        map.to_texture_frame(Vec3::new(d.dot(proj.tangent), d.dot(proj.bitangent), 0.0))
    };
    // A negative scale mirrors the image: its right is then against +u.
    let (right, up) = (
        to_tex(u * f64::from(face.texinfo.u_scale.signum())),
        to_tex(-v * f64::from(face.texinfo.v_scale.signum())),
    );
    let ctx = format!("{:?}", face.texinfo);
    assert!(
        right.x > 0.95 && right.y.abs() < 0.05,
        "image right is not texture +x: {right:?} {ctx}"
    );
    assert!(
        up.y > 0.95 && up.x.abs() < 0.05,
        "image up is not texture +y: {up:?} {ctx}"
    );
}

#[test]
fn bump_frame_follows_rotated_and_mirrored_textures() {
    let level = lit_room(|room| {
        let mut edit = |facing: Vec3d, f: &dyn Fn(&mut TexInfo)| {
            let i = room.plane_facing(facing).expect("plane");
            let mut t = room.texinfos()[i];
            f(&mut t);
            assert!(room.set_texinfo(i, t));
        };
        edit(v3(0.0, 0.0, -1.0), &|t| t.rotate(37.0)); // floor
        edit(v3(-1.0, 0.0, 0.0), &|t| t.flip_u()); // wall seen from +x
        edit(v3(1.0, 0.0, 0.0), &|t| {
            t.rotate(90.0);
            t.flip_v();
        });
        edit(v3(0.0, 1.0, 0.0), &|t| t.rotate(-120.0));
    });
    let r = real_renderer(&level);
    // Visible normals point into the room.
    for n in [
        v3(0.0, 0.0, 1.0),
        v3(1.0, 0.0, 0.0),
        v3(-1.0, 0.0, 0.0),
        v3(0.0, -1.0, 0.0),
        v3(0.0, 1.0, 0.0),
    ] {
        assert_bump_frame_follows_axes(&r, n);
    }
}

#[test]
fn showcase_pictures_for_inspection() {
    let level = mscent_world::showcase_level();
    let r = real_renderer(&level);
    let views = [
        // Room A: big bricks on the west wall, rotated floor tiles.
        (
            "a_floor_and_west_wall",
            View::from_angles(Vec3::new(15.0, 14.0, 4.0), 0.9, -0.35, 0.0),
        ),
        // Room D: shifted, enlarged floor tiles and the rotated west wall.
        (
            "d_floor_and_rotated_wall",
            View::from_angles(Vec3::new(22.0, 38.0, 4.0), 0.7, -0.3, 0.0),
        ),
    ];
    let mut seen = Vec::new();
    for (name, view) in views {
        let mut fb = Framebuffer::new(640, 360).expect("fb");
        r.render(&mut fb, &view, Quality::High, &[], None)
            .expect("render");
        std::fs::write(shots_dir().join(format!("showcase_{name}.png")), png(&fb)).expect("write");
        let distinct: std::collections::HashSet<u32> = fb.pixels().iter().copied().collect();
        assert!(distinct.len() > 1000, "{name}: {} colours", distinct.len());
        seen.push(fb.pixels().to_vec());
    }
    assert_ne!(seen[0], seen[1]);
}

/// The projection every face used before per-face mapping existed.
fn old_projection(pos: Vec3, n: Vec3d) -> (f32, f32) {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    let axes = if az >= ax && az >= ay {
        (0, 1)
    } else if ax >= ay {
        (1, 2)
    } else {
        (0, 2)
    };
    let pick = |a: usize| [pos.x, pos.y, pos.z][a] / 8.0;
    (pick(axes.0), pick(axes.1))
}

#[test]
fn default_mapping_matches_the_previous_renderer_on_real_levels() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    for seed in [1u64, 7, 99] {
        let brushes = mscent_world::generate(seed, &mscent_world::GenParams::default());
        let bsp = compile(&brushes).expect("compiles");
        let faces = extract_faces(&bsp, &brushes);
        let portals = Portals::build(&bsp);
        let scene = Scene::new(
            bsp,
            faces,
            portals,
            MaterialTable::load_dir(&dir).expect("textures"),
        );
        assert!(scene.faces().len() > 20);
        for (face, poly) in scene.faces().iter().zip(scene.polys()) {
            for vert in poly {
                let (u, v) = old_projection(vert.pos, face.plane.normal);
                let off = (vert.uv.x - u).abs().max((vert.uv.y - v).abs());
                assert!(off < 1e-4, "seed {seed}: uv off by {off}");
            }
        }
    }
}
