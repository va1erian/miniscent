use super::*;
use crate::material::Material;
use crate::raster::draw_polygon;
use mscent_math::rng::Rng;
use mscent_world::{GenParams, compile, extract_faces, generate, room_centers};

const W: u32 = 96;
const H: u32 = 72;
const BACKGROUND: u32 = 0xff00_0000;

/// Writes its face id as the pixel colour, so the framebuffer becomes an id
/// buffer.
struct IdShader(u32);

impl SpanShader for IdShader {
    fn begin_span(&mut self, _: &crate::SpanEnd, _: &crate::SpanEnd, _: u32) {}
    fn shade(&self, _: u32, _: u32, _: i32, _: i32) -> u32 {
        self.0
    }
}

fn id_color(index: usize) -> u32 {
    BACKGROUND | (index as u32 + 1)
}

fn materials() -> MaterialTable {
    let mut t = MaterialTable::new();
    for (i, c) in [0xffc0_8060u32, 0xff60_c080, 0xff80_60c0, 0xffc0_c0c0]
        .into_iter()
        .enumerate()
    {
        t.add(Material {
            name: format!("m{i}"),
            gloss: 0,
            texture: Texture::solid(c),
        });
    }
    t
}

struct Level {
    scene: Scene,
    centers: Vec<Vec3>,
}

fn level(seed: u64, params: &GenParams) -> Level {
    let brushes = generate(seed, params);
    let bsp = compile(&brushes).unwrap();
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    let centers = room_centers(seed, params)
        .iter()
        .map(|c| Vec3::new(c.x as f32, c.y as f32, 4.0))
        .collect();
    Level {
        scene: Scene::new(bsp, faces, portals, materials()),
        centers,
    }
}

fn camera(eye: Vec3, yaw: f32, pitch: f32) -> Camera {
    let dir = Vec3::new(
        yaw.cos() * pitch.cos(),
        yaw.sin() * pitch.cos(),
        pitch.sin(),
    );
    Camera::look_at(
        eye,
        eye + dir,
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        500.0,
        W,
        H,
    )
    .unwrap()
}

/// Id buffer of a scene render.
fn scene_ids(scene: &Scene, cam: &Camera) -> (Framebuffer, SceneStats) {
    let white = Texture::solid(0xffff_ffff);
    let mut fb = Framebuffer::new(W, H).unwrap();
    fb.clear(BACKGROUND);
    let stats = scene.render_with(&mut fb, cam, Some(&white), &mut |i, _| {
        IdShader(id_color(i))
    });
    (fb, stats)
}

/// Reference: every front-facing face, in index order, no culling at all.
fn brute_force_ids(scene: &Scene, cam: &Camera) -> Framebuffer {
    let white = Texture::solid(0xffff_ffff);
    let eye = eye_of(cam).to_f64();
    let mut fb = Framebuffer::new(W, H).unwrap();
    fb.clear(BACKGROUND);
    for (i, (face, poly)) in scene.faces.iter().zip(&scene.polys).enumerate() {
        if face.plane.signed_distance(eye) > 0.0 {
            draw_polygon(&mut fb, cam, poly, &white, &mut IdShader(id_color(i)));
        }
    }
    fb
}

fn mismatches(a: &Framebuffer, b: &Framebuffer) -> usize {
    a.pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(x, y)| x != y)
        .count()
}

fn assert_no_duplicates(stats: &SceneStats) {
    let mut ids = stats.drawn.clone();
    ids.sort_unstable();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "a face was drawn twice");
}

#[test]
fn eye_of_inverts_the_view() {
    let eye = Vec3::new(3.0, -7.0, 2.5);
    let e = eye_of(&camera(eye, 0.7, 0.2));
    assert!((e - eye).length() < 1e-3, "{e:?}");
}

#[test]
fn flythrough_matches_brute_force_with_no_doubled_faces() {
    let params = GenParams::default();
    let mut frames = 0;
    let mut empty_frames = 0;
    for seed in [1u64, 2, 3, 7, 42] {
        let lv = level(seed, &params);
        for &center in &lv.centers {
            for k in 0..8 {
                let cam = camera(center, k as f32 * 0.785, -0.1);
                let (ids, stats) = scene_ids(&lv.scene, &cam);
                let reference = brute_force_ids(&lv.scene, &cam);
                assert_eq!(mismatches(&ids, &reference), 0, "seed {seed} view {k}");
                assert_no_duplicates(&stats);
                frames += 1;
                empty_frames += usize::from(stats.leaves_visible < stats.leaves_total);
            }
        }
    }
    assert!(frames >= 5 * 9 * 8);
    assert!(empty_frames > 0, "portals never culled anything");
}

#[test]
fn random_positions_including_solid_ones_still_match() {
    let params = GenParams {
        cols: 4,
        rows: 3,
        extra_links: 3,
        pillar_percent: 80,
    };
    let mut rng = Rng::new(99);
    for seed in [5u64, 11] {
        let lv = level(seed, &params);
        for _ in 0..60 {
            let c = lv.centers[rng.below(lv.centers.len() as u32) as usize];
            let eye = c + Vec3::new(rng.range_f32(-6.0, 6.0), rng.range_f32(-6.0, 6.0), 0.0);
            let cam = camera(
                eye,
                rng.range_f32(0.0, core::f32::consts::TAU),
                rng.range_f32(-0.5, 0.5),
            );
            let (ids, stats) = scene_ids(&lv.scene, &cam);
            assert_eq!(mismatches(&ids, &brute_force_ids(&lv.scene, &cam)), 0);
            assert_no_duplicates(&stats);
        }
    }
}

#[test]
fn portals_and_frustum_cull_work() {
    let params = GenParams {
        cols: 5,
        rows: 5,
        extra_links: 0,
        pillar_percent: 0,
    };
    let lv = level(3, &params);
    let total = lv.scene.faces().len();
    let mut max_drawn = 0;
    for &center in &lv.centers {
        let (_, stats) = scene_ids(&lv.scene, &camera(center, 0.3, 0.0));
        max_drawn = max_drawn.max(stats.drawn.len());
        assert!(stats.nodes_culled > 0 || stats.leaves_visible < stats.leaves_total);
    }
    assert!(max_drawn * 2 < total, "drew {max_drawn} of {total}");
}

#[test]
fn default_render_draws_pixels_and_respects_fog() {
    let lv = level(2, &GenParams::default());
    let cam = camera(lv.centers[0], 0.5, 0.0);
    let mut fb = Framebuffer::new(W, H).unwrap();
    fb.clear_sky(0xff20_4080, 0xff80_a0c0);
    let stats = lv.scene.render(&mut fb, &cam, None);
    assert!(stats.pixels > (W * H) / 2, "{}", stats.pixels);
    let fog = Fog::new(0xffff_ffff, 0.0, 1.0);
    let mut foggy = Framebuffer::new(W, H).unwrap();
    foggy.clear(0xff00_0000);
    lv.scene.render(&mut foggy, &cam, fog);
    let covered = |fb: &Framebuffer| fb.pixels().iter().filter(|&&p| p == 0xffff_ffff).count();
    assert!(covered(&foggy) > covered(&fb));
}

#[test]
fn camera_outside_any_room_does_not_panic() {
    let lv = level(1, &GenParams::default());
    for eye in [
        Vec3::new(-500.0, -500.0, 3.0),
        Vec3::new(1.0e5, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -50.0),
    ] {
        let (_, stats) = scene_ids(&lv.scene, &camera(eye, 1.0, 0.0));
        assert_no_duplicates(&stats);
    }
}
