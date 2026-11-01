use super::*;
use mscent_math::rng::Rng;

const A: u32 = 0xffff_0000;
const B: u32 = 0xff00_ff00;
const C: u32 = 0xff00_00ff;
const D: u32 = 0xffff_ffff;

/// 64x64 target, 90 degree fov (focal length 32 px), camera at the origin
/// looking down -Z.
fn setup() -> (Framebuffer, Camera) {
    let cam = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 1.0, 0.0),
        core::f32::consts::FRAC_PI_2,
        0.1,
        200.0,
        64,
        64,
    )
    .unwrap();
    (Framebuffer::new(64, 64).unwrap(), cam)
}

fn quad(half: f32, z: f32) -> [Vertex; 4] {
    [
        Vertex::new(Vec3::new(-half, half, z), 0.0, 0.0),
        Vertex::new(Vec3::new(half, half, z), 1.0, 0.0),
        Vertex::new(Vec3::new(half, -half, z), 1.0, 1.0),
        Vertex::new(Vec3::new(-half, -half, z), 0.0, 1.0),
    ]
}

fn checker() -> Texture {
    Texture::new(2, 2, vec![A, B, C, D]).unwrap()
}

/// Number of pixels that have been drawn (non-zero stored 1/z).
fn lit(fb: &Framebuffer) -> usize {
    (0..64)
        .flat_map(|y| (0..64).map(move |x| (x, y)))
        .filter(|&(x, y)| fb.depth_at(x, y).is_some_and(|d| d > 0.0))
        .count()
}

#[test]
fn fronto_parallel_quad_maps_texels_exactly() {
    let (mut fb, cam) = setup();
    let st = draw_polygon_flat(&mut fb, &cam, &quad(2.0, -2.0), &checker(), 256, None);
    assert_eq!(st.pixels, 64 * 64);
    assert_eq!(fb.pixel(16, 16), Some(A));
    assert_eq!(fb.pixel(48, 16), Some(B));
    assert_eq!(fb.pixel(16, 48), Some(C));
    assert_eq!(fb.pixel(48, 48), Some(D));
    // Texel boundary falls exactly between pixels 31 and 32.
    assert_eq!(fb.pixel(31, 16), Some(A));
    assert_eq!(fb.pixel(32, 16), Some(B));
    assert_eq!(fb.depth_at(10, 10), Some(0.5));
}

#[test]
fn coverage_is_exact_pixel_centres() {
    let (mut fb, cam) = setup();
    // x,y in [-1, 1] at depth 2 -> screen [16, 48): exactly 32 x 32 pixels.
    let st = draw_polygon_flat(&mut fb, &cam, &quad(1.0, -2.0), &checker(), 256, None);
    assert_eq!(st.pixels, 1024);
    assert!(fb.pixel(15, 20) == Some(0xff00_0000) && fb.pixel(16, 20) != Some(0xff00_0000));
    assert!(fb.pixel(47, 20) != Some(0xff00_0000) && fb.pixel(48, 20) == Some(0xff00_0000));
}

#[test]
fn spans_are_sixteen_pixels_wide() {
    let (mut fb, cam) = setup();
    let st = draw_polygon_flat(&mut fb, &cam, &quad(2.0, -2.0), &checker(), 256, None);
    assert_eq!(st.spans, 64 * 4);
    // 32 wide rows split 16 + 16.
    let (mut fb2, cam2) = setup();
    let st2 = draw_polygon_flat(&mut fb2, &cam2, &quad(1.0, -2.0), &checker(), 256, None);
    assert_eq!(st2.spans, 32 * 2);
}

#[test]
fn two_triangles_sharing_an_edge_cover_each_pixel_once() {
    let (mut fb, cam) = setup();
    let q = quad(1.3, -2.0);
    let t = Texture::solid(A);
    let s1 = draw_polygon_flat(&mut fb, &cam, &[q[0], q[1], q[2]], &t, 256, None);
    let s2 = draw_polygon_flat(&mut fb, &cam, &[q[0], q[2], q[3]], &t, 256, None);
    let whole = {
        let (mut f, c) = setup();
        draw_polygon_flat(&mut f, &c, &q, &t, 256, None).pixels
    };
    // Equal depth never overwrites, so a double-covered pixel would be lost
    // from the second count and a gap from the total: both must add up.
    assert_eq!(s1.pixels + s2.pixels, whole);
    assert_eq!(lit(&fb) as u32, whole);
}

#[test]
fn nearer_polygon_wins_in_either_draw_order() {
    for near_first in [true, false] {
        let (mut fb, cam) = setup();
        let near = Texture::solid(A);
        let far = Texture::solid(B);
        let draws = [(quad(1.0, -2.0), &near), (quad(4.0, -4.0), &far)];
        let order: Vec<_> = if near_first {
            draws.to_vec()
        } else {
            draws.into_iter().rev().collect()
        };
        for (q, t) in order {
            draw_polygon_flat(&mut fb, &cam, &q, t, 256, None);
        }
        assert_eq!(fb.pixel(32, 32), Some(A), "centre belongs to the near quad");
        assert_eq!(
            fb.pixel(8, 32),
            Some(B),
            "outside the near quad the far one shows"
        );
    }
}

#[test]
fn slanted_wall_is_perspective_correct() {
    // Wall from (0,y,-4) to (12,y,-16): depth grows with x. 64-texel-wide
    // strip texture whose texel colour is its own index.
    let strip = Texture::new(64, 1, (0..64).map(|i| 0xff00_0000 | i).collect()).unwrap();
    let wall = [
        Vertex::new(Vec3::new(0.0, 1.0, -4.0), 0.0, 0.0),
        Vertex::new(Vec3::new(12.0, 1.0, -16.0), 12.0 / 64.0, 0.0),
        Vertex::new(Vec3::new(12.0, -1.0, -16.0), 12.0 / 64.0, 1.0),
        Vertex::new(Vec3::new(0.0, -1.0, -4.0), 0.0, 1.0),
    ];
    let (mut fb, cam) = setup();
    draw_polygon_flat(&mut fb, &cam, &wall, &strip, 256, None);
    let row = 32; // inside the wall, which spans a few rows around y = 32
    let tex_at = |px: i32| fb.pixel(px, row).map(|p| (p & 0xff) as i32);
    // Exact at span starts: x = 4 s / (32 - s) for screen offset s.
    assert_eq!(tex_at(32), Some(0));
    assert_eq!(tex_at(48), Some(4));
    // Affine mapping would give about 8 at pixel 48: clearly distinguishable.
    for px in 32..56 {
        let s = px as f32 + 0.5 - 32.0;
        let truth = (4.0 * s / (32.0 - s)).floor() as i32;
        let got = tex_at(px).unwrap();
        assert!((got - truth).abs() <= 2, "px {px}: got {got}, true {truth}");
    }
}

#[test]
fn polygon_crossing_the_near_plane_draws_without_artifacts() {
    // A floor that runs from behind the camera to far ahead.
    let floor = [
        Vertex::new(Vec3::new(-2.0, -1.0, 5.0), 0.0, 0.0),
        Vertex::new(Vec3::new(2.0, -1.0, 5.0), 1.0, 0.0),
        Vertex::new(Vec3::new(2.0, -1.0, -50.0), 1.0, 1.0),
        Vertex::new(Vec3::new(-2.0, -1.0, -50.0), 0.0, 1.0),
    ];
    let (mut fb, cam) = setup();
    let st = draw_polygon_flat(&mut fb, &cam, &floor, &checker(), 256, None);
    assert!(
        st.pixels > 500,
        "floor should fill the lower screen, got {}",
        st.pixels
    );
    assert_eq!(
        fb.pixel(32, 10),
        Some(0xff00_0000),
        "nothing above the horizon"
    );
    assert!(fb.depth_at(32, 63).unwrap() > 0.0);
}

#[test]
fn polygons_entirely_off_screen_or_behind_draw_nothing() {
    let (mut fb, cam) = setup();
    let t = Texture::solid(A);
    assert_eq!(
        draw_polygon_flat(&mut fb, &cam, &quad(1.0, 3.0), &t, 256, None).pixels,
        0
    );
    let mut side = quad(1.0, -2.0);
    for v in &mut side {
        v.pos.x += 100.0;
    }
    assert_eq!(
        draw_polygon_flat(&mut fb, &cam, &side, &t, 256, None),
        DrawStats::default()
    );
    assert_eq!(
        draw_polygon_flat(&mut fb, &cam, &side[..2], &t, 256, None).pixels,
        0
    );
}

#[derive(Default)]
struct Recorder {
    spans: Vec<(SpanEnd, SpanEnd, u32)>,
}

impl SpanShader for Recorder {
    fn begin_span(&mut self, s: &SpanEnd, e: &SpanEnd, len: u32) {
        self.spans.push((*s, *e, len));
    }
    fn shade(&self, i: u32, _texel: u32, _tx: i32, _ty: i32) -> u32 {
        0xff00_0000 | i
    }
}

#[test]
fn shader_hook_sees_span_ends_and_pixel_indices() {
    let (mut fb, cam) = setup();
    let mut rec = Recorder::default();
    draw_polygon(&mut fb, &cam, &quad(2.0, -2.0), &checker(), &mut rec);
    assert_eq!(rec.spans.len(), 256);
    let (s, e, len) = rec.spans[0];
    assert_eq!((s.x, s.y, len), (0, 0, 16));
    assert_eq!(e.x, 16);
    assert!((s.depth - 2.0).abs() < 1e-4 && (e.depth - 2.0).abs() < 1e-4);
    // u advances 16 px * (2 texels / 64 px) = 0.5 texel per span.
    assert!((e.u - s.u - 0.5).abs() < 1e-3);
    // The shader's per-pixel colour is the index within the span.
    assert_eq!(fb.pixel(0, 0), Some(0xff00_0000));
    assert_eq!(fb.pixel(5, 0), Some(0xff00_0005));
    assert_eq!(fb.pixel(17, 0), Some(0xff00_0001));
}

#[test]
fn fog_darkens_with_distance_through_the_flat_shader() {
    let (mut fb, cam) = setup();
    let fog = Fog::new(0xff00_0000, 0.0, 10.0);
    let t = Texture::solid(0xffff_ffff);
    // A floor receding into the distance: rows nearer the horizon are fogged more.
    let floor = [
        Vertex::new(Vec3::new(-5.0, -1.0, -1.0), 0.0, 0.0),
        Vertex::new(Vec3::new(5.0, -1.0, -1.0), 1.0, 0.0),
        Vertex::new(Vec3::new(5.0, -1.0, -30.0), 1.0, 1.0),
        Vertex::new(Vec3::new(-5.0, -1.0, -30.0), 0.0, 1.0),
    ];
    draw_polygon_flat(&mut fb, &cam, &floor, &t, 256, fog);
    let near = fb.pixel(32, 63).unwrap() & 0xff;
    let far = fb.pixel(32, 33).unwrap() & 0xff;
    assert!(near > far, "near {near} should be brighter than far {far}");
    assert!(far < 40, "far floor nearly black, got {far}");
}

#[test]
fn random_polygons_never_panic_or_write_out_of_bounds() {
    let mut rng = Rng::new(11);
    let t = checker();
    let (mut fb, cam) = setup();
    for _ in 0..3000 {
        let n = 3 + rng.below(4) as usize;
        let mut r = |lo: f32, hi: f32| rng.range_f32(lo, hi);
        let poly: Vec<Vertex> = (0..n)
            .map(|_| {
                Vertex::new(
                    Vec3::new(r(-60.0, 60.0), r(-60.0, 60.0), r(-90.0, 30.0)),
                    r(-9.0, 9.0),
                    r(-9.0, 9.0),
                )
            })
            .collect();
        let fog = Fog::new(0xff10_1010, 1.0, 50.0);
        draw_polygon_flat(&mut fb, &cam, &poly, &t, 300, fog);
    }
}
