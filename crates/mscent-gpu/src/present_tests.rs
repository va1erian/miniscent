//! The composite against what the CPU path does to the same frame: the white
//! flash by `lerp_toward`, the layer by `layer_over`, then the platform's own
//! `blit_fit` into the window. A machine with no GPU skips this, unless
//! `MSCENT_REQUIRE_GPU` is set.

use mscent_platform::scale::{blit_fit, fit_rect};
use mscent_render::{Framebuffer, fill_rect, layer_over, simd};

use super::{Compositor, Overlay};
use crate::device::{Gpu, GpuError};
use crate::targets::{COLOR, map};

const FRAME: (u32, u32) = (40, 24);

fn gpu() -> Option<Gpu> {
    match Gpu::new() {
        Ok(gpu) => Some(gpu),
        Err(why @ (GpuError::NoAdapter(_) | GpuError::NoDevice(_))) => {
            assert!(std::env::var_os("MSCENT_REQUIRE_GPU").is_none(), "{why}");
            None
        }
        Err(why) => panic!("{why}"),
    }
}

fn texture(gpu: &Gpu, (w, h): (u32, u32), usage: wgpu::TextureUsages) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: COLOR,
        usage,
        view_formats: &[],
    })
}

/// A scene with every colour different, and a layer of translucent and solid
/// marks over part of it.
fn frame() -> (Framebuffer, Framebuffer) {
    frame_with(FRAME, FRAME)
}

/// [`frame`] with the scene at `scene_size` pixels and the layer at `layer_size`.
fn frame_with(scene_size: (u32, u32), layer_size: (u32, u32)) -> (Framebuffer, Framebuffer) {
    let mut scene = Framebuffer::new(scene_size.0, scene_size.1).expect("a frame");
    for (i, p) in scene.pixels_mut().iter_mut().enumerate() {
        let i = i as u32;
        *p = 0xff00_0000 | (i * 37 % 256) << 16 | (i * 91 % 256) << 8 | (i * 13 % 256);
    }
    let mut layer = Framebuffer::new(layer_size.0, layer_size.1).expect("a layer");
    layer.clear(0);
    fill_rect(&mut layer, 2, 2, 20, 12, 0x80ff_2010);
    fill_rect(&mut layer, 12, 8, 20, 12, 0x4010_ff40);
    fill_rect(&mut layer, 30, 0, 3, 24, 0xffff_ffff);
    (scene, layer)
}

/// What the GPU composites into a window of `window` pixels.
fn composited(
    gpu: &Gpu,
    scene: &Framebuffer,
    layer: &Framebuffer,
    white: u32,
    window: (u32, u32),
) -> Vec<u32> {
    composited_by(
        &mut Compositor::new(gpu, COLOR),
        gpu,
        scene,
        layer,
        (white, window),
    )
}

/// [`composited`] with `compositor`, which keeps its layer from frame to frame.
fn composited_by(
    compositor: &mut Compositor,
    gpu: &Gpu,
    scene: &Framebuffer,
    layer: &Framebuffer,
    (white, window): (u32, (u32, u32)),
) -> Vec<u32> {
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    let size = (scene.width(), scene.height());
    let source = texture(gpu, size, usage);
    let bytes: Vec<u8> = scene
        .pixels()
        .iter()
        .flat_map(|p| p.to_le_bytes())
        .collect();
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(size.0 * 4),
        rows_per_image: None,
    };
    gpu.queue
        .write_texture(source.as_image_copy(), &bytes, layout, source.size());
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    let target = texture(gpu, window, usage);
    let overlay = Overlay {
        layer: layer.pixels(),
        layer_size: (layer.width(), layer.height()),
        white,
        window,
        rect: fit_rect(size.0, size.1, window.0, window.1),
    };
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let view = source.create_view(&Default::default());
    let into = target.create_view(&Default::default());
    compositor
        .encode(gpu, &mut encoder, (&view, size), &overlay, &into)
        .expect("composited");
    let row = (window.0 * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let read = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * window.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(row),
        rows_per_image: None,
    };
    let to = wgpu::TexelCopyBufferInfo {
        buffer: &read,
        layout,
    };
    encoder.copy_texture_to_buffer(target.as_image_copy(), to, target.size());
    gpu.queue.submit([encoder.finish()]);
    map(gpu, &read).expect("read back");
    let bytes = read.get_mapped_range(..).expect("mapped").to_vec();
    bytes
        .chunks_exact(row as usize)
        .flat_map(|r| {
            r[..window.0 as usize * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|&t| u32::from_le_bytes(t))
        })
        .collect()
}

/// What the CPU path shows in the same window.
fn expected(scene: &Framebuffer, layer: &Framebuffer, white: u32, window: (u32, u32)) -> Vec<u32> {
    let mut frame = scene.clone();
    simd::lerp_toward(frame.pixels_mut(), 0xffff_ffff, white);
    layer_over(&mut frame, layer);
    let mut out = vec![0; (window.0 * window.1) as usize];
    blit_fit(
        frame.pixels(),
        FRAME.0,
        FRAME.1,
        &mut out,
        window.0,
        window.1,
    );
    out
}

#[test]
fn the_composite_is_what_the_cpu_path_shows() {
    let Some(gpu) = gpu() else { return };
    let (scene, layer) = frame();
    // Two whole factors, and one that is not, with letterbox at the sides.
    for (white, window) in [
        (0, (80, 48)),
        (100, (120, 72)),
        (0, (100, 50)),
        (256, (100, 50)),
    ] {
        let got = composited(&gpu, &scene, &layer, white, window);
        let want = expected(&scene, &layer, white, window);
        let worst = (got.iter().zip(&want))
            .map(|(&a, &b)| {
                [16, 8, 0]
                    .map(|s| ((a >> s) & 255).abs_diff((b >> s) & 255))
                    .into_iter()
                    .max()
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        eprintln!("white {white} window {window:?}: worst channel {worst}");
        assert!(
            worst <= 2,
            "white {white} window {window:?}: {worst} levels apart"
        );
        assert_eq!(gpu.take_error(), None);
    }
}

/// What the GPU shows of a layer smaller than the scene: the flashed scene as
/// the platform scales it, and the layer over it by whole pixels.
fn expected_smaller(
    scene: &Framebuffer,
    layer: &Framebuffer,
    white: u32,
    window: (u32, u32),
) -> Vec<u32> {
    let mut flashed = scene.clone();
    simd::lerp_toward(flashed.pixels_mut(), 0xffff_ffff, white);
    let mut shown = Framebuffer::new(window.0, window.1).expect("a window");
    let (sw, sh) = (scene.width(), scene.height());
    blit_fit(
        flashed.pixels(),
        sw,
        sh,
        shown.pixels_mut(),
        window.0,
        window.1,
    );
    let (x0, y0, w, h) = fit_rect(sw, sh, window.0, window.1);
    let mut over = Framebuffer::new(window.0, window.1).expect("a window");
    over.clear(0);
    let (lw, lh) = (layer.width(), layer.height());
    for y in 0..h {
        for x in 0..w {
            let (lx, ly) = ((x * lw / w).min(lw - 1), (y * lh / h).min(lh - 1));
            let from = layer.pixels()[(ly * lw + lx) as usize];
            over.pixels_mut()[((y0 + y) * window.0 + x0 + x) as usize] = from;
        }
    }
    layer_over(&mut shown, &over);
    shown.pixels().to_vec()
}

#[test]
fn a_smaller_layer_goes_over_the_scaled_scene_by_whole_pixels() {
    let Some(gpu) = gpu() else { return };
    // The world at the window's size and scaled up to it; the HUD at a half and
    // at a third of the window.
    for (scene, layer, window, white) in [
        ((80, 48), (40, 24), (80, 48), 0),
        ((80, 48), (40, 24), (120, 72), 90),
        ((120, 72), (40, 24), (120, 72), 0),
    ] {
        let (scene, layer) = frame_with(scene, layer);
        let got = composited(&gpu, &scene, &layer, white, window);
        let want = expected_smaller(&scene, &layer, white, window);
        let worst = (got.iter().zip(&want))
            .flat_map(|(&a, &b)| [16, 8, 0].map(|s| ((a >> s) & 255).abs_diff((b >> s) & 255)))
            .max()
            .unwrap_or(0);
        let lsize = (layer.width(), layer.height());
        eprintln!("layer {lsize:?} window {window:?}: worst channel {worst}");
        assert!(
            worst <= 2,
            "layer {lsize:?} window {window:?}: {worst} levels apart"
        );
        assert_eq!(gpu.take_error(), None);
    }
}

#[test]
fn a_layer_whose_marks_move_leaves_none_behind() {
    let Some(gpu) = gpu() else { return };
    let (scene, _) = frame();
    let mut compositor = Compositor::new(&gpu, COLOR);
    let marks = |rows: (i32, i32)| {
        let mut layer = Framebuffer::new(FRAME.0, FRAME.1).expect("a layer");
        layer.clear(0);
        fill_rect(
            &mut layer,
            4,
            rows.0,
            20,
            (rows.1 - rows.0) as u32,
            0xc0ff_ff40,
        );
        layer
    };
    let empty = {
        let mut layer = Framebuffer::new(FRAME.0, FRAME.1).expect("a layer");
        layer.clear(0);
        layer
    };
    // Top rows, then bottom rows, then nothing: each frame shows only its own.
    for layer in [marks((1, 6)), marks((16, 22)), empty] {
        let got = composited_by(&mut compositor, &gpu, &scene, &layer, (0, (80, 48)));
        assert_eq!(got, expected(&scene, &layer, 0, (80, 48)));
    }
    assert_eq!(gpu.take_error(), None);
}
