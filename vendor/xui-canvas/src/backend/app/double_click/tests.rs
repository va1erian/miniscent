//! Unit tests for the pure [`ClickTracker`](super::ClickTracker): the pairing
//! rules, the edge cases and the event sequence the canvas backend produces.

use std::time::Duration;

use super::*;

const TIME: Duration = Duration::from_millis(500);
const DISTANCE: i32 = 4;
const WIDGET: WidgetId = WidgetId::from_raw(7);
const OTHER: WidgetId = WidgetId::from_raw(8);

fn tracker() -> ClickTracker {
    ClickTracker::new(TIME, DISTANCE)
}

#[test]
fn two_presses_within_the_interval_and_distance_double_click() {
    let mut tracker = tracker();
    let at = Instant::now();
    assert_eq!(
        tracker.press(at, 10, 10, MouseButton::Left, WIDGET),
        PressKind::Down
    );
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(200),
            12,
            13,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::DoubleClick
    );
}

#[test]
fn the_interval_and_distance_inclusive_bounds_are_accepted() {
    let mut tracker = tracker();
    let at = Instant::now();
    tracker.press(at, 0, 0, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(at + TIME, DISTANCE, -DISTANCE, MouseButton::Left, WIDGET),
        PressKind::DoubleClick
    );
}

#[test]
fn a_press_one_millisecond_past_the_interval_is_a_new_first_press() {
    let mut tracker = tracker();
    let at = Instant::now();
    tracker.press(at, 10, 10, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(
            at + TIME + Duration::from_millis(1),
            10,
            10,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::Down
    );
}

#[test]
fn a_press_one_pixel_past_the_distance_is_a_new_first_press() {
    let mut tracker = tracker();
    let at = Instant::now();
    tracker.press(at, 10, 10, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(100),
            10 + DISTANCE + 1,
            10,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::Down
    );
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(200),
            10,
            10 + DISTANCE + 1,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::Down
    );
}

#[test]
fn a_different_button_is_not_a_double_click() {
    let mut tracker = tracker();
    let at = Instant::now();
    tracker.press(at, 10, 10, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(at, 10, 10, MouseButton::Right, WIDGET),
        PressKind::Down
    );
}

#[test]
fn a_different_widget_is_not_a_double_click() {
    let mut tracker = tracker();
    let at = Instant::now();
    tracker.press(at, 10, 10, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(at, 10, 10, MouseButton::Left, OTHER),
        PressKind::Down
    );
}

#[test]
fn a_third_quick_press_starts_a_new_sequence() {
    let mut tracker = tracker();
    let at = Instant::now();
    assert_eq!(
        tracker.press(at, 10, 10, MouseButton::Left, WIDGET),
        PressKind::Down
    );
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(100),
            10,
            10,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::DoubleClick
    );
    // The pair consumed the sequence, so the third quick press is a fresh
    // first press, and only a fourth would make another double-click.
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(200),
            10,
            10,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::Down
    );
    assert_eq!(
        tracker.press(
            at + Duration::from_millis(300),
            10,
            10,
            MouseButton::Left,
            WIDGET
        ),
        PressKind::DoubleClick
    );
}

#[test]
fn the_win32_sequence_is_down_up_double_click_up() {
    // The Win32 backend maps a double-click to `MouseDown`, `MouseUp`,
    // `MouseDoubleClick`, `MouseUp` (`WM_LBUTTONDBLCLK` replaces the second
    // `WM_LBUTTONDOWN`). The canvas backend must produce the same.
    let mut tracker = tracker();
    let at = Instant::now();
    let mut sequence = Vec::new();
    for (offset, button) in [(0u64, MouseButton::Left), (150, MouseButton::Left)] {
        let x = 20;
        let y = 30;
        let kind = tracker.press(at + Duration::from_millis(offset), x, y, button, WIDGET);
        sequence.push(kind.event(x, y, button, Modifiers::NONE));
        sequence.push(Event::MouseUp {
            x,
            y,
            button,
            modifiers: Modifiers::NONE,
        });
    }
    assert_eq!(
        sequence,
        vec![
            Event::MouseDown {
                x: 20,
                y: 30,
                button: MouseButton::Left,
                modifiers: Modifiers::NONE,
            },
            Event::MouseUp {
                x: 20,
                y: 30,
                button: MouseButton::Left,
                modifiers: Modifiers::NONE,
            },
            Event::MouseDoubleClick {
                x: 20,
                y: 30,
                button: MouseButton::Left,
                modifiers: Modifiers::NONE,
            },
            Event::MouseUp {
                x: 20,
                y: 30,
                button: MouseButton::Left,
                modifiers: Modifiers::NONE,
            },
        ]
    );
}

#[test]
fn a_negative_distance_is_treated_as_zero() {
    let mut tracker = ClickTracker::new(TIME, -1);
    let at = Instant::now();
    tracker.press(at, 1, 1, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(at, 2, 1, MouseButton::Left, WIDGET),
        PressKind::Down
    );
}

#[test]
fn the_platform_tracker_has_usable_thresholds() {
    // Whatever the platform reports, a press at the same instant, position
    // and widget is always a double-click.
    let mut tracker = ClickTracker::system();
    let at = Instant::now();
    tracker.press(at, 0, 0, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(at, 0, 0, MouseButton::Left, WIDGET),
        PressKind::DoubleClick
    );
}

#[test]
fn x_and_y_limits_are_independent() {
    // A wide, short box: 6px either side horizontally, 1px vertically.
    let mut tracker = ClickTracker::with_extent(TIME, 6, 1);
    let first = Instant::now();
    let _ = tracker.press(first, 0, 0, MouseButton::Left, WIDGET);
    assert_eq!(
        tracker.press(first, 6, 0, MouseButton::Left, WIDGET),
        PressKind::DoubleClick,
        "6px across is inside the box"
    );
    let _ = tracker.press(first, 0, 0, MouseButton::Left, WIDGET);
    assert_ne!(
        tracker.press(first, 0, 2, MouseButton::Left, WIDGET),
        PressKind::DoubleClick,
        "2px down is outside a 1px-high box"
    );
}
