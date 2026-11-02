//! Pictures of voxel volumes beside real brush geometry, before the renderer
//! knows about volumes (M9.1). Triangles go through `draw_polygon` with a light
//! per triangle from its normal, a stand-in for the baked light of M9.5.
//! Fuchsia behind everything shows any hole in a surface, and each picture
//! reports how many such pixels it has.
//!
//! `cargo run --release -p mscent-render --example voxel_shot -- target/shots/voxel`

use std::path::{Path, PathBuf};

use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::png_encode::encode_png;
use mscent_render::scene::Scene;
use mscent_render::{Camera, FlatShader, Fog, Framebuffer, MaterialTable, Vertex, draw_polygon};
use mscent_world::brush::{Brush, Op};
use mscent_world::materials::{BRICK, FLOOR, METAL, ROCK};
use mscent_world::voxel::{
    CellSize, ChunkMesh, Context, DabOp, Lod, Mesher, Prim, Volume, apply_dab, fill,
};
use mscent_world::{Face, Portals, compile, extract_faces};

const SIZE: (u32, u32) = (1280, 720);
const HOLE: u32 = 0xffff_00ff;
const FOG: u32 = 0xff14_1820;
/// The look of each material number these volumes use: samples hold material
/// ids, and these scenes want other pictures than ids 0 to 3 give.
const PALETTE: [u16; 4] = [ROCK, FLOOR, BRICK, METAL];
/// World units one repeat of a texture covers on a voxel surface.
const TILE: f32 = 4.0;

type Failure = Box<dyn std::error::Error>;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cuboid(lo: Vec3d, hi: Vec3d, op: Op, order: u32, material: u16) -> Result<Brush, Failure> {
    let b = Brush::cuboid(lo, hi, op, order).map_err(|e| format!("{e:?}"))?;
    Ok(b.with_material(material))
}

fn dab(volume: &mut Volume, ctx: &mut Context, prim: Option<Prim>, op: DabOp) {
    if let Some(p) = prim {
        apply_dab(volume, ctx, &p, op);
    }
}

/// Every chunk's mesh, at the level `pick` chooses from the mesh's distance.
fn meshes(
    volume: &Volume,
    brushes: &[Brush],
    pick: impl Fn(&ChunkMesh) -> Option<Lod>,
) -> Vec<ChunkMesh> {
    let mut ctx = Context::new(volume, brushes);
    let mut mesher = Mesher::new();
    let [cx, cy, cz] = volume.chunks();
    let mut out = Vec::new();
    for z in 0..cz {
        for y in 0..cy {
            for x in 0..cx {
                let Some(m) = mesher.mesh(volume, &mut ctx, [x, y, z]) else {
                    continue;
                };
                out.push(match pick(&m) {
                    Some(lod) => m.clustered(lod),
                    None => m,
                });
            }
        }
    }
    out
}

/// Draws the back-face-culled triangles of `mesh`, textured through the axis a
/// triangle faces most, which is how the real path will project them too.
fn draw_mesh(
    fb: &mut Framebuffer,
    cam: &Camera,
    scene: &Scene,
    eye: Vec3,
    mesh: &ChunkMesh,
    fog: Option<Fog>,
) {
    let sun = Vec3::new(0.35, -0.5, 0.8)
        .normalized()
        .unwrap_or(Vec3::ZERO);
    let lamp = |to_eye: Vec3, n: Vec3| to_eye.normalized().map_or(0.0, |d| n.dot(d).max(0.0));
    for (t, &material) in mesh.triangles.iter().zip(&mesh.materials) {
        let [a, b, c] = t.map(|i| mesh.vertices[usize::from(i)]);
        let face = (b.pos - a.pos).cross(c.pos - a.pos);
        if face.dot(eye - a.pos) <= 0.0 {
            continue;
        }
        let n = (a.normal + b.normal + c.normal)
            .normalized()
            .unwrap_or(Vec3::ZERO);
        let uv = |p: Vec3| {
            let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
            let (s, q) = if az >= ax && az >= ay {
                (p.x, p.y)
            } else if ay >= ax {
                (p.x, p.z)
            } else {
                (p.y, p.z)
            };
            (s / TILE, q / TILE)
        };
        let light = 70.0 + 110.0 * n.dot(sun).max(0.0) + 130.0 * lamp(eye - a.pos, n);
        let vert = |p: Vec3| {
            let (s, q) = uv(p);
            Vertex::new(p, s, q)
        };
        let poly = [vert(a.pos), vert(b.pos), vert(c.pos)];
        let mut shader = FlatShader::new(light as u32, fog);
        draw_polygon(
            fb,
            cam,
            &poly,
            scene.texture_of(PALETTE[usize::from(material) % PALETTE.len()]),
            &mut shader,
        );
    }
}

/// Draws brushes and voxel meshes from `eye` and writes the picture.
fn shoot(
    scene: &Scene,
    list: &[ChunkMesh],
    eye: Vec3,
    target: Vec3,
    out: &Path,
) -> Result<(), Failure> {
    let (w, h) = SIZE;
    let up = Vec3::new(0.0, 0.0, 1.0);
    let cam = Camera::look_at(eye, target, up, 1.15, 0.05, 400.0, w, h).ok_or("bad camera")?;
    let mut fb = Framebuffer::new(w, h).ok_or("bad size")?;
    fb.clear_sky(HOLE, HOLE);
    let fog = Fog::new(FOG, 6.0, 40.0);
    scene.render(&mut fb, &cam, fog);
    for m in list {
        draw_mesh(&mut fb, &cam, scene, eye, m, fog);
    }
    let holes = fb.pixels().iter().filter(|&&p| p == HOLE).count();
    let tris: usize = list.iter().map(|m| m.triangles.len()).sum();
    let png = encode_png(w, h, fb.pixels()).ok_or("cannot encode")?;
    std::fs::write(out, png)?;
    println!(
        "{}: {tris} voxel triangles, {holes} hole pixels",
        out.display()
    );
    Ok(())
}

/// A scene of `brushes`, without the faces of `shell` if there is one: those
/// are never drawn, as the voxel surface closes in front of them.
fn scene_of(
    brushes: &[Brush],
    shell: Option<&Brush>,
    table: MaterialTable,
) -> Result<Scene, Failure> {
    let bsp = compile(brushes).map_err(|e| format!("{e:?}"))?;
    let mut faces: Vec<Face> = extract_faces(&bsp, brushes);
    if let Some(s) = shell {
        faces.retain(|f| !on_shell(f, s));
    }
    let portals = Portals::build(&bsp);
    Ok(Scene::new(bsp, faces, portals, table))
}

fn on_shell(f: &Face, shell: &Brush) -> bool {
    let near = |p: Vec3d, pl: &mscent_math::DPlane| pl.signed_distance(p).abs() < 1.0e-6;
    shell.planes().iter().any(|pl| {
        let verts = f.polygon.vertices();
        verts.iter().all(|&p| near(p, pl)) && verts.iter().all(|&p| shell.contains(p))
    })
}

/// A cave dug into a brush wall: everything left of the volume's box is the
/// plain brush wall, and the rock inside the box still follows it.
fn cave_entrance(dir: &Path, table: MaterialTable) -> Result<(), Failure> {
    let room = cuboid(
        v(-20.0, -20.0, 0.0),
        v(20.0, 0.0, 10.0),
        Op::Subtract,
        0,
        ROCK,
    )?;
    let brushes = vec![room];
    let mut volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [-32, -8, -8], [4, 4, 3])
        .map_err(|e| format!("{e:?}"))?;
    let mut ctx = Context::new(&volume, &brushes);
    let mut rng = Rng::new(21);
    // The tunnel: a wandering chain of round cones going in through the wall.
    let (mut at, mut radius) = (v(0.0, -1.5, 2.3), 2.3);
    for _ in 0..9 {
        let next = at + v(rng.range_f64(-1.0, 1.0), 1.5, rng.range_f64(-0.3, 0.45));
        let r = (radius + rng.range_f64(-0.3, 0.25)).clamp(1.3, 2.4);
        dab(
            &mut volume,
            &mut ctx,
            Prim::round_cone(at, next, radius, r),
            DabOp::Carve,
        );
        (at, radius) = (next, r);
    }
    // Weathering: lumps and bites around the mouth, then a smoothing pass.
    for _ in 0..70 {
        let p = v(
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-0.3, 5.0),
            rng.range_f64(0.0, 7.0),
        );
        let prim = Prim::sphere(p, rng.range_f64(0.25, 0.7));
        let op = if rng.below(2) == 0 {
            DabOp::Carve
        } else {
            DabOp::Add { material: 0 }
        };
        dab(&mut volume, &mut ctx, prim, op);
    }
    for _ in 0..40 {
        let p = v(
            rng.range_f64(-5.0, 5.0),
            rng.range_f64(-0.5, 7.0),
            rng.range_f64(0.0, 7.0),
        );
        dab(&mut volume, &mut ctx, Prim::sphere(p, 1.6), DabOp::Smooth);
    }
    let shell = volume
        .shell()
        .ok_or("subtract volume")?
        .map_err(|e| format!("{e:?}"))?;
    let mut all = brushes.clone();
    all.push(shell.clone());
    let scene = scene_of(&all, Some(&shell), table)?;
    let list = meshes(&volume, &brushes, |_| None);
    let target = Vec3::new(0.0, 3.0, 2.3);
    shoot(
        &scene,
        &list,
        Vec3::new(-7.5, -11.0, 3.5),
        target,
        &dir.join("cave-entrance.png"),
    )?;
    shoot(
        &scene,
        &list,
        Vec3::new(0.6, -1.0, 2.2),
        Vec3::new(0.5, 5.0, 2.4),
        &dir.join("cave-inside.png"),
    )
}

fn stalagmite(volume: &mut Volume, ctx: &mut Context, rng: &mut Rng, x: f64, y: f64, up: bool) {
    let (root, sign) = if up { (-0.4, 1.0) } else { (10.4, -1.0) };
    let height = rng.range_f64(1.4, 3.8);
    let base = rng.range_f64(0.4, 0.75);
    let mid = v(
        x + rng.range_f64(-0.15, 0.15),
        y + rng.range_f64(-0.15, 0.15),
        root + sign * height * 0.55,
    );
    let tip = v(
        x + rng.range_f64(-0.3, 0.3),
        y + rng.range_f64(-0.3, 0.3),
        root + sign * height,
    );
    let m = DabOp::Add { material: 0 };
    dab(
        volume,
        ctx,
        Prim::round_cone(v(x, y, root), mid, base, base * 0.5),
        m,
    );
    dab(volume, ctx, Prim::round_cone(mid, tip, base * 0.5, 0.2), m);
}

fn mushroom(volume: &mut Volume, ctx: &mut Context, at: Vec3d, size: f64) {
    let top = at + v(0.15 * size, 0.0, 1.9 * size);
    let cap = Prim::sphere(top, 1.15 * size);
    dab(volume, ctx, cap, DabOp::Add { material: 2 });
    // Cutting the sphere's underside leaves a cap; the stem then goes in.
    let under = Prim::sphere(top - v(0.0, 0.0, 0.75 * size), 1.05 * size);
    dab(volume, ctx, under, DabOp::Carve);
    let stem = Prim::round_cone(at, top, 0.42 * size, 0.24 * size);
    dab(volume, ctx, stem, DabOp::Add { material: 1 });
    dab(volume, ctx, Prim::sphere(at, 0.6 * size), DabOp::Smooth);
}

fn coral(
    volume: &mut Volume,
    ctx: &mut Context,
    rng: &mut Rng,
    at: Vec3d,
    dir: Vec3d,
    r: f64,
    depth: u32,
) {
    let next = at + dir * (r * rng.range_f64(2.6, 3.8));
    let end = (r * 0.78).max(0.17);
    dab(
        volume,
        ctx,
        Prim::round_cone(at, next, r, end),
        DabOp::Add { material: 1 },
    );
    if depth == 0 {
        return;
    }
    for _ in 0..2 {
        let turn = v(
            rng.range_f64(-0.8, 0.8),
            rng.range_f64(-0.8, 0.8),
            rng.range_f64(0.1, 0.7),
        );
        let bent = (dir + turn).normalized().unwrap_or(dir);
        coral(volume, ctx, rng, next, bent, end, depth - 1);
    }
}

/// A brush room with things grown in it, in an add volume.
fn garden(dir: &Path, table: MaterialTable) -> Result<(), Failure> {
    let floor = cuboid(
        v(-12.0, -12.0, 0.0),
        v(12.0, 12.0, 10.0),
        Op::Subtract,
        0,
        METAL,
    )?;
    let brushes = vec![floor.clone()];
    let mut volume = Volume::new(Op::Add, 1, CellSize::Quarter, [-40, -40, -4], [5, 5, 3])
        .map_err(|e| format!("{e:?}"))?;
    let mut ctx = Context::new(&volume, &brushes);
    let mut rng = Rng::new(4);
    for _ in 0..9 {
        let (x, y) = (rng.range_f64(-9.0, 9.0), rng.range_f64(0.5, 9.0));
        stalagmite(&mut volume, &mut ctx, &mut rng, x, y, true);
    }
    for _ in 0..13 {
        let (x, y) = (rng.range_f64(-9.0, 9.0), rng.range_f64(-3.0, 9.0));
        stalagmite(&mut volume, &mut ctx, &mut rng, x, y, false);
    }
    for (x, y, s) in [
        (-3.4, -3.2, 1.0),
        (-1.6, -4.4, 0.65),
        (-4.8, -1.4, 0.75),
        (-5.3, -4.5, 0.5),
    ] {
        mushroom(&mut volume, &mut ctx, v(x, y, -0.1), s);
    }
    for (x, y) in [(3.4, -3.0), (5.6, -0.6), (1.6, 0.6)] {
        coral(
            &mut volume,
            &mut ctx,
            &mut rng,
            v(x, y, -0.2),
            v(0.0, 0.0, 1.0),
            0.5,
            4,
        );
    }
    let scene = scene_of(&brushes, None, table)?;
    let list = meshes(&volume, &brushes, |_| None);
    shoot(
        &scene,
        &list,
        Vec3::new(-0.5, -10.5, 2.9),
        Vec3::new(0.0, 0.0, 1.9),
        &dir.join("garden.png"),
    )
}

/// A warped gyroid, positive solid.
fn alien_field(p: Vec3d) -> f64 {
    let g = |x: f64, y: f64, z: f64| x.sin() * y.cos() + y.sin() * z.cos() + z.sin() * x.cos();
    g(p.x * 0.55, p.y * 0.55, p.z * 0.55)
        + g(p.x * 1.7 + 3.0, p.y * 1.7, p.z * 1.7 + 1.0) * 0.18
        + g(p.x * 4.1, p.y * 4.3 + 2.0, p.z * 3.9) * 0.05
        + 0.25
}

/// How far a line of sight runs from `from` along `dir` before it meets rock.
fn clear_run(from: Vec3d, dir: Vec3d) -> f64 {
    (1..120)
        .map(|i| f64::from(i) * 0.25)
        .find(|&t| alien_field(from + dir * t) >= 0.0)
        .unwrap_or(30.0)
}

/// A seeded look around for the place in the cave with the longest view: an
/// eye in rock would see the backs of the surfaces, and through them.
fn best_view() -> (Vec3, Vec3) {
    let mut rng = Rng::new(8);
    let mut best = (0.0, Vec3d::ZERO, Vec3d::ZERO);
    for _ in 0..4000 {
        let eye = v(
            rng.range_f64(8.0, 24.0),
            rng.range_f64(8.0, 24.0),
            rng.range_f64(5.0, 11.0),
        );
        if alien_field(eye) > -0.3 {
            continue;
        }
        let turn = v(
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-0.3, 0.3),
        );
        let Some(dir) = turn.normalized() else {
            continue;
        };
        let run = clear_run(eye, dir);
        if run > best.0 {
            best = (run, eye, dir);
        }
    }
    (best.1.to_f32(), (best.1 + best.2 * 4.0).to_f32())
}

/// Inside a gyroid cave with no brushes near, all at once and with level of
/// detail: both must have no holes.
fn alien_cave(dir: &Path, table: MaterialTable) -> Result<(), Failure> {
    let brushes = vec![cuboid(
        v(100.0, 100.0, 100.0),
        v(104.0, 104.0, 104.0),
        Op::Subtract,
        0,
        BRICK,
    )?];
    let mut volume = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [8, 8, 4])
        .map_err(|e| format!("{e:?}"))?;
    let mut ctx = Context::new(&volume, &brushes);
    fill(&mut volume, &mut ctx, alien_field, 3);
    let scene = scene_of(&brushes, None, table)?;
    let (eye, target) = best_view();
    let full = meshes(&volume, &brushes, |_| None);
    shoot(&scene, &full, eye, target, &dir.join("alien-cave.png"))?;
    let by_distance = |m: &ChunkMesh| {
        let d = (m.bounds.center() - eye).length();
        match d {
            d if d > 16.0 => Some(Lod::Quarter),
            d if d > 8.0 => Some(Lod::Half),
            _ => None,
        }
    };
    let reduced = meshes(&volume, &brushes, by_distance);
    shoot(
        &scene,
        &reduced,
        eye,
        target,
        &dir.join("alien-cave-lod.png"),
    )
}

fn main() -> Result<(), Failure> {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("target/shots/voxel"), PathBuf::from);
    std::fs::create_dir_all(&dir)?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let table = || MaterialTable::load_dir(&root).map_err(|e| format!("{e:?}"));
    cave_entrance(&dir, table()?)?;
    garden(&dir, table()?)?;
    alien_cave(&dir, table()?)
}
