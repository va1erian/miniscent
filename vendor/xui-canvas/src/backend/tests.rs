#![forbid(unsafe_code)]

//! Unit tests for the window backend that do not need a live window.

use super::{Decorations, PlatformSpec, WindowState};
use xui_core::Image;
use xui_core::units::dip;

#[test]
fn window_state_records_the_requested_chrome() {
    let spec = PlatformSpec::new("t")
        .decorations(Decorations::None)
        .caption_inset(dip(36.0));
    let state = WindowState::new(&spec);
    assert_eq!(state.decorations, Decorations::None);
    assert_eq!(state.caption_inset, dip(36.0));
}

#[test]
fn a_default_window_keeps_its_decorations() {
    let state = WindowState::new(&PlatformSpec::new("t"));
    assert_eq!(state.decorations, Decorations::System);
    assert_eq!(state.caption_inset, dip(0.0));
}

#[test]
fn window_state_records_whether_the_window_is_resizable() {
    assert!(WindowState::new(&PlatformSpec::new("t")).resizable);
    let mut spec = PlatformSpec::new("t");
    spec.resizable = false;
    assert!(!WindowState::new(&spec).resizable);
}

#[test]
fn a_window_that_does_not_exist_yet_remembers_its_icon() {
    let mut state = WindowState::new(&PlatformSpec::new("t"));
    assert!(
        state.winit_icon().is_none(),
        "no icon until the app sets one"
    );
    state.icon = Some(Image::from_rgba(2, 2, vec![0xFF; 16]).expect("image"));
    assert!(
        state.winit_icon().is_some(),
        "the pending icon converts when the window is created"
    );
}

#[test]
fn an_image_becomes_a_winit_icon() {
    let image = Image::from_rgba(4, 4, vec![0x80; 64]).expect("image");
    assert!(super::to_winit_icon(&image).is_some());
}
