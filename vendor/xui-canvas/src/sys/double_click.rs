//! The system's double-click interval and distance.
//!
//! `winit` does not report double-clicks, so the canvas backend synthesizes
//! them from the press sequence and needs the platform's thresholds. Windows
//! exposes the user's configured values as `GetDoubleClickTime` and
//! `SM_CXDOUBLECLK`/`SM_CYDOUBLECLK` (`Winuser.h`); every other platform has no
//! such query here and uses the documented fallback.

use std::time::Duration;

/// The double-click interval used when the platform has no query, in
/// milliseconds.
pub(crate) const FALLBACK_TIME: Duration = Duration::from_millis(500);

/// How far the second press may land from the first on each axis when the
/// platform has no query, in device pixels.
pub(crate) const FALLBACK_DISTANCE: i32 = 4;

/// The platform's double-click interval and its allowed offset from the first
/// press, per axis (device pixels): `(time, (dx, dy))`.
///
/// On Windows these are the user's configured values, falling back when the
/// system reports none; elsewhere they are [`FALLBACK_TIME`] and
/// [`FALLBACK_DISTANCE`] on both axes, since `winit` exposes no query.
#[cfg(windows)]
pub(crate) fn system() -> (Duration, (i32, i32)) {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXDOUBLECLK, SM_CYDOUBLECLK,
    };

    // SAFETY: both entry points take no pointers and return plain integers;
    // they have no preconditions and cannot fail for these indices.
    let milliseconds = unsafe { GetDoubleClickTime() };
    let (wide, high) = unsafe {
        (
            GetSystemMetrics(SM_CXDOUBLECLK),
            GetSystemMetrics(SM_CYDOUBLECLK),
        )
    };
    let time = if milliseconds == 0 {
        FALLBACK_TIME
    } else {
        Duration::from_millis(u64::from(milliseconds))
    };
    (time, (half_extent(wide), half_extent(high)))
}

/// The allowed offset on one axis for a Win32 double-click rectangle `extent`
/// wide: the rectangle is centred on the first press, so each side gets half.
#[cfg(windows)]
fn half_extent(extent: i32) -> i32 {
    if extent > 0 {
        extent / 2
    } else {
        FALLBACK_DISTANCE
    }
}

/// The documented fallback, for platforms whose `winit` backend exposes no
/// double-click query.
#[cfg(not(windows))]
pub(crate) fn system() -> (Duration, (i32, i32)) {
    (FALLBACK_TIME, (FALLBACK_DISTANCE, FALLBACK_DISTANCE))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn the_win32_rectangle_is_centred_so_each_side_gets_half() {
        assert_eq!(half_extent(4), 2, "the Windows default 4px box is +-2px");
        assert_eq!(half_extent(9), 4);
        assert_eq!(half_extent(0), FALLBACK_DISTANCE, "no value falls back");
        assert_eq!(half_extent(-3), FALLBACK_DISTANCE);
    }
}
