use super::*;
use crate::camera::Camera;
use crate::raster::{Vertex, draw_polygon_flat};
use crate::shade::Fog;
use crate::texture::Texture;
use mscent_math::Vec3;
use mscent_math::rng::Rng;

const W: u32 = 97;
const H: u32 = 70;

struct Scene {
    cam: Camera,
    tex: Texture,
    tris: Vec<([Vertex; 3], u32)>,
    fog: Option<Fog>,
}

fn scene(seed: u64) -> Scene {
    let mut rng = Rng::new(seed);
    let cam = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        W,
        H,
    )
    .unwrap();
    let albedo = (0..16 * 16)
        .map(|_| rng.next_u64() as u32 | 0xff00_0000)
        .collect();
    let tex = Texture::new(16, 16, albedo).unwrap();
    let vertex = |rng: &mut Rng| {
        Vertex::new(
            Vec3::new(
                rng.range_f32(-9.0, 9.0),
                rng.range_f32(2.0, 25.0),
                rng.range_f32(-7.0, 7.0),
            ),
            rng.range_f32(0.0, 3.0),
            rng.range_f32(0.0, 3.0),
        )
    };
    // Overlapping triangles make the depth test decide pixels across tiles.
    let tris = (0..80)
        .map(|_| {
            let tri = [vertex(&mut rng), vertex(&mut rng), vertex(&mut rng)];
            (tri, 64 + rng.below(300))
        })
        .collect();
    Scene {
        cam,
        tex,
        tris,
        fog: Fog::new(0xff30_4050, 5.0, 40.0),
    }
}

fn draw_scene(s: &Scene, fb: &mut Framebuffer) {
    fb.clear_sky(0xff10_2030, 0xff80_9070);
    for (tri, light) in &s.tris {
        draw_polygon_flat(fb, &s.cam, tri, &s.tex, *light, s.fog);
    }
}

fn reference(s: &Scene) -> Framebuffer {
    let mut fb = Framebuffer::new(W, H).unwrap();
    draw_scene(s, &mut fb);
    fb
}

fn tiled(s: &Scene, tile_rows: u32, workers: usize) -> Framebuffer {
    let mut r = TileRenderer::new(W, H, tile_rows, workers).unwrap();
    let mut fb = Framebuffer::new(W, H).unwrap();
    r.render(&mut fb, &|band| draw_scene(s, band)).unwrap();
    fb
}

fn assert_same(a: &Framebuffer, b: &Framebuffer, what: &str) {
    assert!(a.pixels() == b.pixels(), "colour differs: {what}");
    let depth = |f: &Framebuffer| {
        (0..H as i32)
            .flat_map(|y| (0..W as i32).map(move |x| (x, y)))
            .map(|(x, y)| f.depth_at(x, y).unwrap().to_bits())
            .collect::<Vec<_>>()
    };
    assert!(depth(a) == depth(b), "depth differs: {what}");
}

#[test]
fn scene_actually_draws_something() {
    let fb = reference(&scene(1));
    let sky = Framebuffer::new(W, H).map(|mut f| {
        f.clear_sky(0xff10_2030, 0xff80_9070);
        f
    });
    assert!(fb.pixels() != sky.unwrap().pixels());
}

#[test]
fn one_and_many_workers_give_identical_pixels() {
    for seed in 1..=4 {
        let s = scene(seed);
        let one = tiled(&s, 16, 1);
        for workers in [2, 3, 8, 200] {
            assert_same(&one, &tiled(&s, 16, workers), &format!("{workers} workers"));
        }
    }
}

#[test]
fn tiled_output_equals_the_untiled_frame() {
    let s = scene(9);
    let full = reference(&s);
    // Tile heights that divide the height, do not divide it, and exceed it.
    for rows in [1, 7, 16, 35, 70, 500] {
        assert_same(&full, &tiled(&s, rows, 3), &format!("tile_rows {rows}"));
    }
}

#[test]
fn rendering_twice_into_the_same_target_is_repeatable() {
    let s = scene(5);
    let mut r = TileRenderer::new(W, H, 8, 4).unwrap();
    let mut fb = Framebuffer::new(W, H).unwrap();
    r.render(&mut fb, &|band| draw_scene(&s, band)).unwrap();
    let first = fb.clone();
    r.set_workers(1);
    r.render(&mut fb, &|band| draw_scene(&s, band)).unwrap();
    assert_same(&first, &fb, "second frame");
}

#[test]
fn constructor_and_render_reject_bad_sizes() {
    assert!(TileRenderer::new(0, 10, 4, 1).is_none());
    assert!(TileRenderer::new(10, 10, 0, 1).is_none());
    let mut r = TileRenderer::new(10, 10, 4, 0).unwrap();
    assert_eq!(r.workers(), 1);
    assert_eq!(r.tile_count(), 3);
    r.set_workers(10_000);
    assert_eq!(r.workers(), MAX_WORKERS);
    let mut wrong = Framebuffer::new(10, 11).unwrap();
    assert_eq!(r.render(&mut wrong, &|_| {}), Err(SizeMismatch));
}
