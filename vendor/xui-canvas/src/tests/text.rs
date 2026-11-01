use super::*;

#[test]
fn text_rasterises_and_writes_a_png() {
    let mut surface = Surface::new(360, 96);
    surface.fill(Color::hex(0xF3F3F3));
    surface.with_canvas(Rect::new(0, 0, 360, 96), |canvas| {
        let style = TextStyle::new(Color::hex(0x1B_1B_1B), Dip(24.0)).middle();
        canvas.draw_text("Hello xui", Rect::new(16, 16, 344, 80), &style);
    });

    let image = surface.to_image();
    assert!(
        dark_pixels(&image) > 100,
        "the text painted dark pixels: {}",
        dark_pixels(&image)
    );
    save("canvas-text.png", &image);
}

#[test]
fn text_scales_with_dpi() {
    let style = TextStyle::new(Color::hex(0x1B_1B_1B), Dip(16.0));
    let at_96 = measure_text("Hello xui", &style, 96, 1000).width;
    let at_192 = measure_text("Hello xui", &style, 192, 1000).width;
    assert!(at_96 > 0, "the text measured a width");
    assert!(
        at_192 > at_96,
        "200% text is wider than 100%: {at_96} vs {at_192}"
    );

    // A side-by-side capture for visual inspection.
    let mut surface = Surface::new(480, 128);
    surface.fill(Color::hex(0xF3F3F3));
    surface.with_canvas_at(Rect::new(0, 0, 480, 64), 96, |canvas| {
        canvas.draw_text("Hello 100%", Rect::new(12, 8, 468, 56), &style);
    });
    surface.with_canvas_at(Rect::new(0, 64, 480, 128), 192, |canvas| {
        canvas.draw_text("Hello 200%", Rect::new(12, 72, 468, 120), &style);
    });
    save("canvas-hidpi.png", &surface.to_image());
}

#[test]
fn a_shaped_layout_has_a_baseline_inside_its_first_line() {
    use xui_core::backend::FontSpec;

    let backend = OffscreenBackend::new();
    let shape = |size| {
        backend
            .text_shaper()
            .layout("Hello", &FontSpec::new(Dip(size)), f32::INFINITY, 96)
    };
    let (small, big) = (shape(16.0), shape(32.0));
    assert!(
        small.baseline() > 0.0 && small.baseline() < small.height(),
        "baseline {} inside height {}",
        small.baseline(),
        small.height()
    );
    assert!(big.baseline() > small.baseline(), "the baseline scales");
}
