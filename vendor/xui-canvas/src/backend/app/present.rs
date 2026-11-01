#![forbid(unsafe_code)]

//! Frame presentation: the shared compositor repaints the damaged part of the
//! window's persistent frame (the whole of it when GL content is live or the
//! frame is new), and only the repainted rows are converted and handed to
//! `softbuffer`.
//!
//! **Not run by any test.** A real window cannot be opened in the
//! environments this code is developed and checked in (an event loop and a
//! display are required), so the present path below is compile-checked on
//! Windows, Linux and macOS and reviewed, not run. What is run (offscreen, in
//! the differential tests) is everything it depends on: damage tracking, the
//! region repaint and the frame's pixels. What is not: `present_with_damage`
//! and the buffer-age handling, whose rules are stated in the comments.

use std::num::NonZeroU32;

use xui_core::geometry::Rect;

use super::super::render::WindowGl;
use super::App;
use crate::Surface;
use crate::compositor::{self, Content as _, FrameInfo};

impl App<'_> {
    /// Presents one frame of `raw`.
    ///
    /// The window's persistent frame is repainted from its damage (every pixel
    /// stays a whole-window repaint's), then handed to `softbuffer`:
    ///
    /// - if the buffer still holds the previous frame (`Buffer::age() == 1`,
    ///   which `softbuffer` reports for the Win32 and X11 backends once a
    ///   buffer of the current size has been presented, and 0 or more for the
    ///   others), only the repainted rectangles are converted and presented
    ///   with `present_with_damage`;
    /// - otherwise (a new or resized buffer, an age of 0 or of 2 or more, or a
    ///   frame that was painted but could not be handed over) the whole frame
    ///   is converted and presented. The persistent frame always holds every
    ///   pixel, so this costs a conversion, not a repaint.
    ///
    /// A window with GL content repaints whole every frame: the GL widget is
    /// rendered per frame and composited through the same frame, so its
    /// changes are not something damage can see.
    pub(super) fn redraw(&mut self, raw: u64) {
        let window_id = Self::window_id(raw);
        let (width, height, handle, dpi, theme) = match self.shared.windows.borrow().get(&raw) {
            Some(state) => (
                state.size.0,
                state.size.1,
                state.window.clone(),
                state.dpi,
                state.theme,
            ),
            None => return,
        };
        let (Some(width), Some(height)) = (NonZeroU32::new(width), NonZeroU32::new(height)) else {
            return;
        };

        let Some(real) = self.windows.get_mut(&raw) else {
            return;
        };
        if !handle
            .as_ref()
            .is_some_and(|handle| real.software.ensure(handle))
        {
            return;
        }
        if real.software.sized(width, height).is_none() {
            return;
        }

        // The persistent frame follows the window's size; a new one is all
        // stale.
        let mut renewed = false;
        let frame = real.frame.get_or_insert_with(|| {
            renewed = true;
            Surface::new(width.get(), height.get())
        });
        if frame.size() != (width.get(), height.get()) {
            *frame = Surface::new(width.get(), height.get());
            renewed = true;
        }

        let window_rect = Rect::new(0, 0, width.get() as i32, height.get() as i32);
        let content = WindowGl {
            shared: &self.shared,
            window: window_id,
        };
        let areas = self.shared.nodes.borrow_mut().take_damage(
            window_id,
            window_rect,
            renewed || content.is_live(),
            compositor::margin(dpi),
        );
        if !areas.is_empty() {
            let plan = compositor::plan(&self.shared.nodes.borrow(), window_id, dpi, &content);
            let info = FrameInfo {
                window: window_rect,
                dpi,
                theme: &theme,
            };
            plan.paint(frame, &areas, &info, &content);
        }

        let Some(software) = real.software.sized(width, height) else {
            real.buffer_stale = true;
            return;
        };
        let Ok(mut buffer) = software.buffer_mut() else {
            // The frame is painted; the buffer does not have it yet.
            real.buffer_stale = true;
            return;
        };
        let sent = rects_to_send(buffer.age(), real.buffer_stale, &areas, &window_rect);
        let Some(frame) = real.frame.as_ref() else {
            return;
        };
        convert(frame, &mut buffer, width.get() as usize, sent);

        let damage: Vec<softbuffer::Rect> = sent
            .iter()
            .filter_map(|rect| {
                Some(softbuffer::Rect {
                    x: u32::try_from(rect.left).ok()?,
                    y: u32::try_from(rect.top).ok()?,
                    width: NonZeroU32::new(u32::try_from(rect.width()).ok()?)?,
                    height: NonZeroU32::new(u32::try_from(rect.height()).ok()?)?,
                })
            })
            .collect();
        // Nothing repainted (an expose, or damage that fell outside the
        // window): the OS may need the whole buffer again, so present it. So
        // does a window where an expose cannot be told from a redraw of our
        // own (see `presents_in_parts`): the buffer holds the whole frame, and
        // only the rows that changed were converted.
        let in_parts = handle.as_deref().is_some_and(presents_in_parts);
        let presented = if damage.is_empty() || !in_parts {
            buffer.present()
        } else {
            buffer.present_with_damage(&damage)
        };
        real.buffer_stale = presented.is_err();
    }
}

/// Converts the rows of `rects` of `frame` into `buffer`, `width` pixels a
/// row. `softbuffer` wants `0x00RRGGBB`; the frame is premultiplied RGBA over
/// an opaque background, so the channels are final.
fn convert(frame: &Surface, buffer: &mut [u32], width: usize, rects: &[Rect]) {
    let pixels = frame.pixels();
    for rect in rects {
        let (left, right) = (rect.left.max(0) as usize, rect.right.max(0) as usize);
        if right <= left {
            continue;
        }
        for row in rect.top.max(0) as usize..rect.bottom.max(0) as usize {
            let from = &pixels[(row * width + left) * 4..(row * width + right) * 4];
            let to = &mut buffer[row * width + left..row * width + right];
            for (destination, pixel) in to.iter_mut().zip(from.as_chunks::<4>().0) {
                *destination =
                    (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
            }
        }
    }
}

/// The rectangles of the frame to convert and present. Only the repainted
/// ones when the buffer still holds the previous frame (`age == 1`, and no
/// frame was lost since); the whole window otherwise: a buffer of unknown age
/// (0), an older one (2 or more) or one that missed a frame cannot be patched.
fn rects_to_send<'a>(
    age: u8,
    buffer_stale: bool,
    damage: &'a [Rect],
    window: &'a Rect,
) -> &'a [Rect] {
    if buffer_stale || age != 1 {
        std::slice::from_ref(window)
    } else {
        damage
    }
}

/// Whether `window` may be handed only the rectangles that changed.
///
/// On X11 without a compositing manager the server clears a region that was
/// covered and exposed, and winit reports the expose as an ordinary
/// `RedrawRequested`: a redraw that coincides with damage cannot be told from
/// our own, so presenting only the damage would leave the exposed area blank.
/// There the whole buffer is presented (still after converting only the rows
/// that changed). Elsewhere the window's contents persist (Windows composites
/// through DWM; Wayland and macOS keep the surface's buffer).
fn presents_in_parts(window: &winit::window::Window) -> bool {
    use winit::raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    !matches!(
        window.window_handle().map(|handle| handle.as_raw()),
        Ok(RawWindowHandle::Xlib(_) | RawWindowHandle::Xcb(_))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_buffer_one_frame_old_is_patched_with_the_damage() {
        let window = Rect::new(0, 0, 100, 80);
        let damage = [Rect::new(10, 10, 20, 20)];
        assert_eq!(rects_to_send(1, false, &damage, &window), damage);
        assert_eq!(rects_to_send(0, false, &damage, &window), [window]);
        assert_eq!(rects_to_send(2, false, &damage, &window), [window]);
        assert_eq!(rects_to_send(255, false, &damage, &window), [window]);
        assert_eq!(
            rects_to_send(1, true, &damage, &window),
            [window],
            "a frame the buffer missed"
        );
    }

    #[test]
    fn nothing_damaged_on_a_current_buffer_sends_nothing_to_convert() {
        let window = Rect::new(0, 0, 100, 80);
        assert!(rects_to_send(1, false, &[], &window).is_empty());
    }

    #[test]
    fn conversion_writes_only_the_asked_rows_in_xrgb() {
        let mut surface = Surface::new(4, 3);
        surface.fill(xui_core::Color::rgb(10, 20, 30));
        let mut buffer = vec![0xDEAD_BEEF_u32; 12];
        convert(&surface, &mut buffer, 4, &[Rect::new(1, 1, 3, 2)]);
        let want = (10 << 16) | (20 << 8) | 30;
        for (at, value) in buffer.iter().enumerate() {
            let (x, y) = (at % 4, at / 4);
            let inside = (1..3).contains(&x) && y == 1;
            assert_eq!(
                *value,
                if inside { want } else { 0xDEAD_BEEF },
                "({x}, {y})"
            );
        }
    }
}
