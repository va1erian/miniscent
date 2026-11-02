//! The seams another renderer is built on: a frame described in one value, a
//! level that outlives its renderer, and visibility with no rasterizer in it.

use std::path::Path;

use mscent_math::Vec3;
use mscent_render::{
    BuildConfig, Effects, FrameInput, FrameRenderer, Framebuffer, LevelRenderer, MaterialTable,
    Quality, View,
};
use mscent_world::showcase_level;

const W: u32 = 160;
const H: u32 = 90;

fn renderer() -> LevelRenderer {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/textures");
    let materials = MaterialTable::load_dir(&dir).expect("textures");
    LevelRenderer::from_level(&showcase_level(), materials, &BuildConfig::default())
        .expect("showcase builds")
}

fn view() -> View {
    View::from_angles(Vec3::new(3.0, 3.0, 2.5), -0.78, 0.0, 0.0)
}

/// Draws through the trait, as a host that does not know the renderer does.
fn drawn(r: &mut dyn FrameRenderer, actors: &[mscent_render::Actor]) -> Vec<u32> {
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let view = view();
    let input = FrameInput {
        view: &view,
        quality: Quality::High,
        actors,
        effects: Effects::default(),
    };
    r.draw(&mut fb, &input).expect("a frame");
    fb.pixels().to_vec()
}

#[test]
fn a_frame_through_the_trait_is_the_frame_of_render() {
    let mut r = renderer();
    let actors = r.actors().to_vec();
    let mut fb = Framebuffer::new(W, H).expect("fb");
    r.render(&mut fb, &view(), Quality::High, &actors, None)
        .expect("a frame");
    assert_eq!(drawn(&mut r, &actors), fb.pixels());
}

#[test]
fn a_level_handed_to_a_new_renderer_draws_the_same_frame() {
    let mut first = renderer();
    let actors = first.actors().to_vec();
    let before = drawn(&mut first, &actors);
    let mut second = LevelRenderer::from_assets(first.into_assets());
    assert_eq!(drawn(&mut second, &actors), before);
}

#[test]
fn the_visible_set_names_the_faces_a_frame_lists() {
    let r = renderer();
    let cam = r.camera(&view(), W, H).expect("a camera");
    let mut chunks = Vec::new();
    let visible = r.scene().visible_set(&cam, &mut chunks);
    let mut fb = Framebuffer::new(W, H).expect("fb");
    let stats = r
        .render(&mut fb, &view(), Quality::Low, &[], None)
        .expect("a frame");
    assert!(!visible.drawn.is_empty());
    assert_eq!(visible.drawn, stats.drawn);
}
