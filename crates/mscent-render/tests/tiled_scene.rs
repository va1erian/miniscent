//! The real level renderer through the tile renderer: every worker count and
//! tile height must give the pixels of the untiled frame.

use mscent_math::Vec3;
use mscent_render::scene::Scene;
use mscent_render::{Camera, Fog, Framebuffer, MaterialTable, TileRenderer};
use mscent_world::{GenParams, Portals, compile, extract_faces, generate, room_centers};

const W: u32 = 160;
const H: u32 = 90;

fn level() -> (Scene, Vec3) {
    let params = GenParams::default();
    let brushes = generate(1, &params);
    let bsp = compile(&brushes).unwrap();
    let faces = extract_faces(&bsp, &brushes);
    let portals = Portals::build(&bsp);
    let eye = room_centers(1, &params)[0].to_f32();
    (Scene::new(bsp, faces, portals, MaterialTable::new()), eye)
}

fn camera(eye: Vec3, yaw: f32) -> Camera {
    let target = eye + Vec3::new(yaw.cos(), yaw.sin(), 0.0) * 10.0;
    Camera::look_at(eye, target, Vec3::new(0.0, 0.0, 1.0), 1.2, 0.1, 400.0, W, H).unwrap()
}

fn draw(scene: &Scene, cam: &Camera, fb: &mut Framebuffer) {
    fb.clear_sky(0xff30_4860, 0xff80_8890);
    scene.render(fb, cam, Fog::new(0xff20_2830, 30.0, 200.0));
}

#[test]
fn tiled_level_matches_untiled_for_all_worker_counts() {
    let (scene, eye) = level();
    for view in 0..4 {
        let cam = camera(eye, view as f32 * 1.7 + 0.3);
        let mut full = Framebuffer::new(W, H).unwrap();
        draw(&scene, &cam, &mut full);
        let background = {
            let mut sky = Framebuffer::new(W, H).unwrap();
            sky.clear_sky(0xff30_4860, 0xff80_8890);
            sky
        };
        assert!(
            full.pixels() != background.pixels(),
            "view {view} drew nothing"
        );
        for (rows, workers) in [(90, 1), (16, 1), (16, 4), (7, 3), (1, 8)] {
            let mut r = TileRenderer::new(W, H, rows, workers).unwrap();
            let mut fb = Framebuffer::new(W, H).unwrap();
            r.render(&mut fb, &|band| draw(&scene, &cam, band)).unwrap();
            assert!(
                fb.pixels() == full.pixels(),
                "view {view}, {rows} rows, {workers} workers"
            );
        }
    }
}
