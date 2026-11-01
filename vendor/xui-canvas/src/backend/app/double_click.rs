#![forbid(unsafe_code)]

//! Double-click synthesis for the canvas backend.
//!
//! `winit` reports every press as a plain `MouseInput` and never reports a
//! double-click, so the backend recognizes one itself. This is the pure part:
//! it remembers the previous press and decides whether the current one
//! completes a double-click, given the system's interval and distance. The
//! platform query for those two values lives in [`crate::sys::double_click`].

use std::time::{Duration, Instant};

use xui_core::backend::{Event, WidgetId};
use xui_core::message::{Modifiers, MouseButton};

/// What a press turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PressKind {
    /// A plain press: the caller delivers [`Event::MouseDown`].
    Down,
    /// The second press of a double-click: the caller delivers
    /// [`Event::MouseDoubleClick`] in place of the second `MouseDown`, matching
    /// the Win32 backend, where `WM_*BUTTONDBLCLK` replaces the second
    /// `WM_*BUTTONDOWN`.
    DoubleClick,
}

impl PressKind {
    /// The event this press delivers, at `(x, y)` in the widget's client
    /// coordinates.
    pub(super) fn event(self, x: i32, y: i32, button: MouseButton, modifiers: Modifiers) -> Event {
        match self {
            PressKind::Down => Event::MouseDown {
                x,
                y,
                button,
                modifiers,
            },
            PressKind::DoubleClick => Event::MouseDoubleClick {
                x,
                y,
                button,
                modifiers,
            },
        }
    }
}

/// One remembered press, in window (not node-local) coordinates: the same
/// widget and a nearby pointer, within the platform's double-click interval.
#[derive(Clone, Copy, Debug)]
struct Press {
    at: Instant,
    x: i32,
    y: i32,
    button: MouseButton,
    target: WidgetId,
}

/// Recognizes the second press of a double-click from the sequence of presses
/// a window receives.
///
/// A press pairs with the one before it when it repeats its button and widget
/// within [`time`](Self::new) and [`distance`](Self::new). A pair then restarts
/// the sequence, so a third quick press is a fresh first press rather than a
/// second double-click.
pub(super) struct ClickTracker {
    time: Duration,
    /// How far the second press may land from the first, on x and on y.
    distance: (i32, i32),
    last: Option<Press>,
}

impl ClickTracker {
    /// A tracker that pairs presses no further apart than `time` and no more
    /// than `distance` device pixels from each other on either axis. A
    /// negative `distance` is treated as zero.
    #[cfg(test)]
    pub(super) fn new(time: Duration, distance: i32) -> ClickTracker {
        ClickTracker::with_extent(time, distance, distance)
    }

    /// Like [`ClickTracker::new`], with separate limits on x (`dx`) and y
    /// (`dy`): the second press must land in the rectangle they span around
    /// the first, as Win32's `SM_CXDOUBLECLK` by `SM_CYDOUBLECLK` box does.
    pub(super) fn with_extent(time: Duration, dx: i32, dy: i32) -> ClickTracker {
        ClickTracker {
            time,
            distance: (dx.max(0), dy.max(0)),
            last: None,
        }
    }

    /// A tracker using the platform's configured double-click interval and
    /// distance, or a documented fallback where the platform has no query.
    pub(super) fn system() -> ClickTracker {
        let (time, (dx, dy)) = crate::sys::double_click::system();
        ClickTracker::with_extent(time, dx, dy)
    }

    /// Classifies a press of `button` on `target` at `(x, y)` in window
    /// coordinates at `now`.
    ///
    /// `Modifiers` are deliberately not part of the pairing: Windows'
    /// double-click detection ignores them too.
    pub(super) fn press(
        &mut self,
        now: Instant,
        x: i32,
        y: i32,
        button: MouseButton,
        target: WidgetId,
    ) -> PressKind {
        let repeats = self
            .last
            .is_some_and(|last| self.matches(&last, now, x, y, button, target));
        self.last = if repeats {
            None
        } else {
            Some(Press {
                at: now,
                x,
                y,
                button,
                target,
            })
        };
        if repeats {
            PressKind::DoubleClick
        } else {
            PressKind::Down
        }
    }

    /// Whether a press repeats `last`.
    fn matches(
        &self,
        last: &Press,
        now: Instant,
        x: i32,
        y: i32,
        button: MouseButton,
        target: WidgetId,
    ) -> bool {
        last.button == button
            && last.target == target
            && now.saturating_duration_since(last.at) <= self.time
            && within(x, last.x, self.distance.0)
            && within(y, last.y, self.distance.1)
    }
}

/// Whether `a` and `b` are no further apart than `distance`, using `i64` so a
/// coordinate pair straddling `i32::MIN`/`i32::MAX` cannot overflow.
fn within(a: i32, b: i32, distance: i32) -> bool {
    (i64::from(a) - i64::from(b)).abs() <= i64::from(distance)
}

#[cfg(test)]
mod tests;
