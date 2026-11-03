//! Parallax relief on faces, read from real pixels. Nothing here compares with the
//! software renderer, which draws none: each look is held to what a brute-force march
//! of the same height map says, to the geometry of two eyes, and to its own frame with
//! the effect off.
//!
//! A machine with no GPU skips this, unless `MSCENT_REQUIRE_GPU` is set.

#[path = "support/gpu.rs"]
mod gpu_support;
#[path = "support/relief.rs"]
mod relief;

use std::ops::Range;

use mscent_gpu::{Finish, Gpu, GpuEffects, GpuRenderer};
use mscent_math::Vec3;
use mscent_render::{Effects, FrameInput, Framebuffer, Quality, View};
use relief::{Floor, Heights, SIDE, decode, draw, looking, room, table};

const SIZE: (u32, u32) = (320, 180);
/// Texels the relief is deep.
const DEPTH: u8 = 6;
/// The texel rate a pixel may have for the relief to be whole: below `PARALLAX_FULL`.
const WHOLE_RATE: f32 = 0.7;

/// An egg crate: highest at (4, 4) and (12, 12) of every 16 texels, lowest between.
fn bumps() -> Heights {
    Box::new(|x, y| {
        let wave = |t: u32| (t as f32 * std::f32::consts::TAU / 16.0).sin();
        (127.5 + 127.5 * wave(x) * wave(y)).round() as u8
    })
}

fn near_eye() -> View {
    looking(Vec3::new(1.2, 4.0, -1.0), -0.65)
}

/// A frame of the room with `heights` from `view`, with parallax as given.
fn frame(
    gpu: &Gpu,
    (heights, depth): (&Heights, u8),
    (view, size): (&View, (u32, u32)),
    parallax: bool,
) -> Vec<u32> {
    let level = room(table(heights, depth, false));
    let effects = GpuEffects {
        shadows: false,
        parallax,
    };
    draw(gpu, &level, view, size, effects)
}

/// How far a frame's pixels are from the marched truth, over a sample of the floor.
struct Errors {
    count: u32,
    mean: f32,
    /// How many read more than 1.5 texels from the truth.
    far: u32,
    worst: f32,
}

impl std::fmt::Debug for Errors {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self {
            count,
            mean,
            far,
            worst,
        } = self;
        write!(
            f,
            "{count} px, mean {mean:.2}, {far} over 1.5, worst {worst:.2}"
        )
    }
}

/// The floor pixels of `frame` that the truth can be read for, and how far from
/// `truth` the texel each shows is.
fn errors(
    floor: &Floor,
    (frame, grey): (&[u32], &[u32]),
    truth: impl Fn(Vec3) -> (f32, f32),
) -> Errors {
    let side = SIDE as f32;
    let (mut count, mut sum, mut worst, mut far) = (0u32, 0.0f32, 0.0f32, 0u32);
    for py in (0..SIZE.1).step_by(3) {
        for px in (0..SIZE.0).step_by(3) {
            let (sx, sy) = (px as f32 + 0.5, py as f32 + 0.5);
            let Some(p) = floor.point_under(sx, sy) else {
                continue;
            };
            if floor.texel_rate(sx, sy) > Some(WHOLE_RATE) {
                continue;
            }
            let want = truth(p);
            let want = (want.0.rem_euclid(side), want.1.rem_euclid(side));
            let at = (py * SIZE.0 + px) as usize;
            // A texel near the edge of the texture blends with the one that wraps.
            let safe = |t: f32| (2.0..side - 2.0).contains(&t);
            let shown = decode(frame[at], grey[at]);
            let Some(shown) = shown.filter(|_| safe(want.0) && safe(want.1)) else {
                continue;
            };
            let apart = |a: f32, b: f32| (a - b).abs().min(side - (a - b).abs());
            let err = apart(shown.0, want.0).max(apart(shown.1, want.1));
            (count, sum, worst) = (count + 1, sum + err, worst.max(err));
            far += u32::from(err > 1.5);
        }
    }
    Errors {
        count,
        mean: sum / count.max(1) as f32,
        far,
        worst,
    }
}

#[test]
fn pixels_show_the_texel_a_brute_force_march_finds() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let (coded, grey) = (
        room(table(&bumps(), DEPTH, false)),
        room(table(&bumps(), DEPTH, true)),
    );
    let view = near_eye();
    let on = draw(&gpu, &coded, &view, SIZE, GpuEffects::at(Quality::High));
    let off = draw(&gpu, &coded, &view, SIZE, GpuEffects::OFF);
    let light = draw(&gpu, &grey, &view, SIZE, GpuEffects::OFF);
    let floor = Floor::new(&coded, &view, SIZE);
    let heights = bumps();
    let marched = |p| floor.marched(&heights, DEPTH, p, 4096);
    let with = errors(&floor, (&on, &light), marched);
    let without = errors(&floor, (&off, &light), marched);
    let unmoved = errors(&floor, (&off, &light), |p| floor.texel_at(p.x, p.y));
    assert!(with.count > 1000, "{} pixels read", with.count);
    // A step count can step over a sliver of a crest, so a few pixels read far.
    assert!(
        with.mean < 0.4 && with.far * 100 < with.count,
        "on {with:?}"
    );
    // The same reading of a frame with the offset zeroed is far from the truth, so
    // this test is one that an offset of zero fails; and the reading is exact when
    // the truth is the unshifted point, so it is the march that it tests.
    assert!(
        without.mean > 2.0 && without.far * 3 > without.count,
        "off {without:?}"
    );
    assert!(unmoved.worst < 1.0, "geometry {unmoved:?}");
}

/// A frame of the bumps at `quality`, with no switch set: the quality decides.
fn by_quality(gpu: &Gpu, view: &View, quality: Quality) -> Vec<u32> {
    let level = room(table(&bumps(), DEPTH, false));
    let finish = Finish::Reference;
    let mut renderer = GpuRenderer::with_finish(gpu.clone(), level.assets(), finish).expect("fits");
    let mut fb = Framebuffer::new(SIZE.0, SIZE.1).expect("a framebuffer");
    let input = FrameInput {
        view,
        quality,
        actors: &[],
        effects: Effects::default(),
        wants_depth: false,
    };
    renderer
        .draw(level.assets(), &mut fb, &input)
        .expect("a frame");
    fb.pixels().to_vec()
}

#[test]
fn parallax_off_and_a_depth_of_nothing_draw_alike_and_quality_picks_the_switch() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let (view, bumps) = (near_eye(), bumps());
    let off_deep = frame(&gpu, (&bumps, DEPTH), (&view, SIZE), false);
    let on_none = frame(&gpu, (&bumps, 0), (&view, SIZE), true);
    let on_deep = frame(&gpu, (&bumps, DEPTH), (&view, SIZE), true);
    assert!(off_deep == on_none, "a depth of 0 draws no relief");
    assert!(
        off_deep != on_deep,
        "the relief is there to be switched off"
    );
    assert!(
        by_quality(&gpu, &view, Quality::High) == on_deep,
        "High draws it"
    );
    assert!(
        by_quality(&gpu, &view, Quality::Medium) == off_deep,
        "Medium does not"
    );
}

#[test]
fn two_eyes_see_a_feature_where_the_geometry_puts_it() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    // The whole floor lies the relief's full depth below the face, but for a
    // plateau at the face, far from the marker in texels.
    let plateau: Heights = Box::new(|x, y| if x < 8 && y < 8 { 255 } else { 0 });
    let marker: Window = (40.0..46.0, 40.0..46.0);
    let eyes = [Vec3::new(1.2, 4.0, -1.0), Vec3::new(3.2, 4.0, -1.0)];
    let mut shift = [0.0f32; 2];
    for (i, eye) in eyes.into_iter().enumerate() {
        let view = looking(eye, -0.65);
        let coded = room(table(&plateau, DEPTH, false));
        let floor = Floor::new(&coded, &view, SIZE);
        let grey = room(table(&plateau, DEPTH, true));
        let light = draw(&gpu, &grey, &view, SIZE, GpuEffects::OFF);
        let reach = f32::from(DEPTH) * floor.world_per_texel;
        let sunk = floor.screen_of_texel((43.0, 43.0), reach).expect("in view");
        let at_plane = floor.screen_of_texel((43.0, 43.0), 0.0).expect("in view");
        for (parallax, want) in [(true, sunk), (false, at_plane)] {
            let shown = frame(&gpu, (&plateau, DEPTH), (&view, SIZE), parallax);
            let found = centroid(&shown, &light, &marker, want)
                .unwrap_or_else(|| panic!("eye {i}, parallax {parallax}: no marker near {want:?}"));
            let miss = (found.0 - want.0).hypot(found.1 - want.1);
            assert!(
                miss < 1.5,
                "eye {i}, parallax {parallax}: {miss:.2} pixels off"
            );
            if parallax {
                shift[i] = (found.0 - at_plane.0).hypot(found.1 - at_plane.1);
            }
        }
    }
    // Each eye found the marker where its own geometry puts it, and the relief moved
    // it by several pixels from where the plane has it, so a march that did nothing
    // is not what these eyes saw.
    assert!(shift[0] > 3.0 && shift[1] > 3.0, "{shift:?}");
}

type Window = (Range<f32>, Range<f32>);

/// The mean pixel of the texels in `marker` shown within 16 pixels of `near`.
fn centroid(frame: &[u32], grey: &[u32], marker: &Window, near: (f32, f32)) -> Option<(f32, f32)> {
    let (mut n, mut sx, mut sy) = (0.0f32, 0.0f32, 0.0f32);
    for py in 0..SIZE.1 {
        for px in 0..SIZE.0 {
            let (cx, cy) = (px as f32 + 0.5, py as f32 + 0.5);
            if (cx - near.0).abs() > 16.0 || (cy - near.1).abs() > 16.0 {
                continue;
            }
            let at = (py * SIZE.0 + px) as usize;
            let texel = decode(frame[at], grey[at]);
            if texel.is_some_and(|(u, v)| marker.0.contains(&u) && marker.1.contains(&v)) {
                (n, sx, sy) = (n + 1.0, sx + cx, sy + cy);
            }
        }
    }
    (n >= 20.0).then(|| (sx / n, sy / n))
}

#[test]
fn a_flat_height_map_draws_as_no_parallax() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let view = near_eye();
    // Flat at the face's plane, and flat lower: both are one plane, which a march
    // counts as the face (the map is lifted so that its highest texel is 255).
    for level in [255u8, 100, 1] {
        let flat: Heights = Box::new(move |_, _| level);
        let on = frame(&gpu, (&flat, DEPTH), (&view, SIZE), true);
        let off = frame(&gpu, (&flat, DEPTH), (&view, SIZE), false);
        assert!(on == off, "flat at {level} moved the texture");
    }
}

#[test]
fn the_light_stays_where_the_face_is() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    // One grey texel everywhere, so relief can move nothing that shows but the
    // light, which is baked on the face and must not slide with the texture.
    let (view, bumps) = (near_eye(), bumps());
    let grey = room(table(&bumps, DEPTH, true));
    let on = draw(&gpu, &grey, &view, SIZE, GpuEffects::at(Quality::High));
    let off = draw(&gpu, &grey, &view, SIZE, GpuEffects::OFF);
    assert!(on == off, "the light moved with the relief");
}

#[test]
fn distant_relief_is_gone() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    let (view, bumps) = (near_eye(), bumps());
    // Seen across a handful of pixels, the texture runs past PARALLAX_GONE texels a pixel.
    let small = (16, 9);
    let on = frame(&gpu, (&bumps, DEPTH), (&view, small), true);
    let off = frame(&gpu, (&bumps, DEPTH), (&view, small), false);
    assert!(on == off, "the relief is drawn where its map aliases");
    // The same view at full size has relief, so the equality is the fade's.
    let near_on = frame(&gpu, (&bumps, DEPTH), (&view, SIZE), true);
    let near_off = frame(&gpu, (&bumps, DEPTH), (&view, SIZE), false);
    assert!(near_on != near_off);
}

#[test]
fn a_glancing_view_has_no_hole_in_the_floor() {
    let Some(gpu) = gpu_support::gpu() else {
        return;
    };
    // An eye a few tenths above the floor, looking along it: the march's rays are
    // nearly flat there, and each one has to land on a texel.
    let view = looking(Vec3::new(1.2, -10.0, -3.7), -0.01);
    let bumps = bumps();
    let coded = room(table(&bumps, DEPTH, false));
    let grey = room(table(&bumps, DEPTH, true));
    let shown = draw(&gpu, &coded, &view, SIZE, GpuEffects::at(Quality::High));
    let light = draw(&gpu, &grey, &view, SIZE, GpuEffects::OFF);
    let floor = Floor::new(&coded, &view, SIZE);
    let mut floor_pixels = 0;
    for py in 0..SIZE.1 {
        for px in 0..SIZE.0 {
            let at = (py * SIZE.0 + px) as usize;
            if floor
                .point_under(px as f32 + 0.5, py as f32 + 0.5)
                .is_none()
                || light[at] & 0xffffff < 0x202020
            {
                continue;
            }
            floor_pixels += 1;
            let (r, g) = (shown[at] >> 16 & 0xff, shown[at] >> 8 & 0xff);
            assert!(r + g > 0, "a hole at {px},{py}: {:08x}", shown[at]);
        }
    }
    assert!(floor_pixels > 2000, "{floor_pixels} floor pixels");
}
