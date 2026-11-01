//! In-memory font registration: the shaper can be fed bundled bytes with no
//! system font directory and no file memory-mapping.
//!
//! Each test runs on a fresh thread because the shaper (and its font registry)
//! is a thread-local: registering fonts only affects a thread whose shaper has
//! not been created yet.

use xui_core::{Canvas, Color, Dip, Rect, TextStyle};

use crate::{Surface, add_font, measure_text, set_default_family, set_default_font};

/// A run in the default family, black at 16px.
fn style() -> TextStyle {
    TextStyle::new(Color::rgb(0, 0, 0), Dip(16.0))
}

/// The bytes of any system font. Used only to seed the in-memory path with a
/// real face; the existing text snapshot tests already assume the host has one.
fn a_real_font() -> Vec<u8> {
    use cosmic_text::fontdb::{Database, Source};

    let mut db = Database::new();
    db.load_system_fonts();
    for face in db.faces() {
        let bytes = match &face.source {
            Source::File(path) => std::fs::read(path).ok(),
            Source::SharedFile(_, data) => Some(data.as_ref().as_ref().to_vec()),
            Source::Binary(data) => Some(data.as_ref().as_ref().to_vec()),
        };
        if let Some(bytes) = bytes {
            return bytes;
        }
    }
    panic!("the host has no system font to seed the in-memory shaper");
}

/// Runs `f` on a fresh thread, so its shaper starts with no fonts registered.
fn on_a_fresh_shaper<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::spawn(f)
        .join()
        .expect("the shaper test thread does not panic")
}

#[test]
fn a_font_registered_from_memory_shapes_text() {
    let bytes = a_real_font();
    let width = on_a_fresh_shaper(move || {
        set_default_font(bytes);
        measure_text("Hello xui", &style(), 96, i32::MAX).width
    });
    assert!(width > 0, "the in-memory face shaped the run: {width}");
}

#[test]
fn registering_the_same_font_twice_still_shapes() {
    let bytes = a_real_font();
    let width = on_a_fresh_shaper(move || {
        add_font(bytes.clone());
        add_font(bytes);
        measure_text("Hello xui", &style(), 96, i32::MAX).width
    });
    assert!(width > 0, "a duplicate face is harmless: {width}");
}

#[test]
fn an_unknown_default_family_falls_back_to_a_loaded_face() {
    let bytes = a_real_font();
    let width = on_a_fresh_shaper(move || {
        set_default_font(bytes);
        set_default_family("No Such Family 12345");
        measure_text("Hello xui", &style(), 96, i32::MAX).width
    });
    assert!(
        width > 0,
        "an unknown family falls back rather than blanking the text: {width}"
    );
}

#[test]
fn invalid_font_bytes_do_not_panic_and_skip_the_system_fonts() {
    // If the system directories were scanned, a host with installed fonts would
    // shape this run. Registering bytes disables that scan, and the invalid
    // bytes add no face, so nothing can shape it.
    let width = on_a_fresh_shaper(|| {
        add_font(vec![0u8; 64]);
        measure_text("Hello xui", &style(), 96, i32::MAX).width
    });
    assert_eq!(width, 0, "no face was loaded, so nothing shaped");
}

#[test]
fn drawing_with_no_usable_font_is_a_no_op() {
    let background = Color::rgb(255, 255, 255);
    let painted = on_a_fresh_shaper(move || {
        add_font(vec![0xFFu8; 32]);
        let mut surface = Surface::new(64, 32);
        surface.fill(background);
        surface.with_canvas(Rect::new(0, 0, 64, 32), |canvas| {
            canvas.draw_text("Hello xui", Rect::new(0, 0, 64, 32), &style());
        });
        surface.pixels().to_vec()
    });

    let bg = [background.r, background.g, background.b, 255];
    assert!(
        painted.as_chunks::<4>().0.iter().all(|pixel| *pixel == bg),
        "with no face there is nothing to paint, and no panic"
    );
}
