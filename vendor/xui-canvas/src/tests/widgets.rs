use super::*;
use xui_core::arrange::{LayoutExt, absolute, button, checkbox, label, progress, slider};

/// An app with nothing to update; the root layout owns the widgets.
struct Widgets;

impl App for Widgets {
    type Msg = u32;
    fn update(&mut self, _msg: u32, _ui: &mut Ui<u32>) {}
}

fn contains(image: &RgbaImage, color: [u8; 3]) -> bool {
    image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| pixel[..3] == color)
}

#[test]
fn portable_widgets_render_on_the_software_backend() {
    let backend = Rc::new(OffscreenBackend::new());
    let backend_for_run: Rc<dyn Backend> = backend.clone();
    let accent = Color::hex(0x00_5F_B8);
    let mut captured: Option<RgbaImage> = None;

    let _ = run_app(
        backend_for_run,
        PlatformSpec::new("canvas widgets").size(Dip(420.0), Dip(200.0)),
        |ui| {
            ui.root(
                absolute().children((
                    button("Click").at(20, 20, 160, 32),
                    checkbox("Enabled").checked(true).at(20, 64, 220, 28),
                    progress(100).value(60).at(20, 104, 360, 8),
                    slider(0.0, 100.0)
                        .then(|slider| {
                            slider.set_value(40.0);
                            slider
                        })
                        .at(20, 128, 360, 28),
                    label("xui on tiny-skia").at(20, 168, 360, 28),
                )),
            )
            .unwrap();

            let image = backend.render(ui.window()).expect("a rendered window");
            assert!(
                contains(&image, [accent.r, accent.g, accent.b]),
                "the accent colour is on the surface"
            );
            save("canvas-widgets.png", &image);
            captured = Some(image);

            Widgets
        },
    );

    assert!(captured.is_some(), "the surface was rendered");
}

#[test]
fn a_design_length_is_converted_once_at_each_dpi() {
    let backend = OffscreenBackend::new();
    // A 10 DIP square is 10px at 100% and 20px at 200%. If the boundary scaled
    // it twice it would cover 40px at 200%, so probing 25px catches that.
    for (dpi, covered, outside) in [(96u32, 5u32, 15u32), (192u32, 15u32, 25u32)] {
        let window = backend
            .open_window_at(&PlatformSpec::new("dpi"), dpi)
            .unwrap();
        assert_eq!(backend.dpi(window), dpi, "the window renders at {dpi}");

        let node = backend
            .create(
                ParentRef::Window(window),
                &NodeSpec::new(NodeKind::Custom, backend.client_rect(window)),
            )
            .unwrap();
        backend.set_painter(
            node,
            Rc::new(move |canvas| {
                let side = Dip(10.0).to_px(canvas.dpi()).value();
                canvas.fill_rect(Rect::new(0, 0, side, side), Color::rgb(255, 0, 0));
            }),
        );

        let image = backend.render(window).expect("a rendered window");
        assert_eq!(
            image.pixel(covered, covered),
            Some([255, 0, 0, 255]),
            "a 10 DIP square covers {covered}px at {dpi} DPI"
        );
        assert_ne!(
            image.pixel(outside, outside),
            Some([255, 0, 0, 255]),
            "and not {outside}px, so the DIP was scaled exactly once"
        );
    }
}
