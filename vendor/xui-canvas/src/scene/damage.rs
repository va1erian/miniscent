#![forbid(unsafe_code)]

//! A window's damage: what changed since the last painted frame, as a short
//! list of rectangles.
//!
//! Every change to a node tells the scene which window rectangles it can have
//! changed ([`Damage::add`]); the next frame repaints only those. The list is
//! kept short so planning a frame stays proportional to the number of nodes
//! (each rectangle is one repaint of every node it meets):
//!
//! - a rectangle already covered by one in the list is dropped;
//! - two rectangles merge when their union is no bigger than the two together
//!   (they overlap heavily or sit side by side), so a burst of tiny adjacent
//!   damages becomes one rectangle, while a thin horizontal rule and a thin
//!   vertical one stay two thin rectangles instead of a window-sized union;
//! - the list never holds more than [`MAX_RECTS`]: past that, the two
//!   rectangles whose union wastes the least area are merged.
//!
//! A full invalidate (a first frame, a resize, a theme change) replaces the
//! list with "everything" and is the fallback whenever the bounded list would
//! not help.

use xui_core::geometry::Rect;

use crate::compositor::grow;
use crate::geometry::{intersect, union};

/// The most rectangles a window's damage keeps before it merges the closest.
pub(crate) const MAX_RECTS: usize = 8;

/// What changed in one window since its last painted frame.
#[derive(Debug, Default)]
pub(crate) struct Damage {
    rects: Vec<Rect>,
    full: bool,
}

/// The area of `rect`, in pixels (0 when empty).
fn area(rect: Rect) -> i64 {
    if rect.is_empty() {
        0
    } else {
        i64::from(rect.width()) * i64::from(rect.height())
    }
}

fn contains(outer: Rect, inner: Rect) -> bool {
    outer.left <= inner.left
        && outer.top <= inner.top
        && outer.right >= inner.right
        && outer.bottom >= inner.bottom
}

/// Whether the union of `a` and `b` is no bigger than both together, so
/// merging them repaints nothing worth mentioning twice.
fn cheap_union(a: Rect, b: Rect) -> bool {
    area(union(a, b)) <= area(a) + area(b)
}

impl Damage {
    /// A window nothing has been painted in yet: everything is stale.
    pub(crate) fn full() -> Damage {
        Damage {
            rects: Vec::new(),
            full: true,
        }
    }

    /// Whether any frame is owed.
    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.full || !self.rects.is_empty()
    }

    /// The rectangles waiting, for tests and diagnostics. Meaningless when
    /// [`Damage::is_full`].
    #[cfg(test)]
    pub(crate) fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// Whether everything is stale.
    #[cfg(test)]
    pub(crate) fn is_full(&self) -> bool {
        self.full
    }

    /// Marks the whole window stale.
    pub(crate) fn add_all(&mut self) {
        self.full = true;
        self.rects.clear();
    }

    /// Marks `rect` (window coordinates) stale. The overdraw margin is added
    /// when the damage is taken ([`Damage::take`]), at the DPI of that frame.
    pub(crate) fn add(&mut self, rect: Rect) {
        if self.full || rect.is_empty() {
            return;
        }
        // Edges are kept far inside the range where widths and areas overflow.
        let rect = grow(rect, 0);
        if self.rects.iter().any(|held| contains(*held, rect)) {
            return;
        }
        let mut merged = rect;
        // A union can start to meet a rectangle the parts did not: go on until
        // nothing more joins.
        loop {
            let before = self.rects.len();
            self.rects.retain(|held| {
                if cheap_union(*held, merged) {
                    merged = union(*held, merged);
                    false
                } else {
                    true
                }
            });
            if self.rects.len() == before {
                break;
            }
        }
        self.rects.push(merged);
        while self.rects.len() > MAX_RECTS {
            self.merge_closest();
        }
    }

    /// Merges the two rectangles whose union adds the least area.
    fn merge_closest(&mut self) {
        let mut best: Option<(usize, usize, i64)> = None;
        for first in 0..self.rects.len() {
            for second in first + 1..self.rects.len() {
                let (a, b) = (self.rects[first], self.rects[second]);
                let waste = area(union(a, b)) - area(a) - area(b);
                if best.is_none_or(|(_, _, least)| waste < least) {
                    best = Some((first, second, waste));
                }
            }
        }
        if let Some((first, second, _)) = best {
            let merged = union(self.rects[first], self.rects[second]);
            self.rects.swap_remove(second);
            self.rects[first] = merged;
        }
    }

    /// Takes the damage: the rectangles to repaint, each grown by `margin`
    /// pixels (the overdraw margin at the window's DPI) and trimmed to
    /// `window`, or the whole window when everything is stale (or `force` is
    /// set, or the list already covers the window). Empty when nothing changed.
    pub(crate) fn take(&mut self, window: Rect, force: bool, margin: i32) -> Vec<Rect> {
        let full = self.full || force;
        let rects = std::mem::take(&mut self.rects);
        self.full = false;
        if full {
            return vec![window];
        }
        let mut clipped: Vec<Rect> = rects
            .into_iter()
            .map(|rect| intersect(grow(rect, margin), window))
            .filter(|rect| !rect.is_empty())
            .collect();
        if clipped.iter().map(|rect| area(*rect)).sum::<i64>() >= area(window) {
            clipped = vec![window];
        }
        clipped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> Rect {
        Rect::new(0, 0, 1000, 800)
    }

    #[test]
    fn a_new_window_is_all_stale_and_taking_clears_it() {
        let mut damage = Damage::full();
        assert!(damage.is_pending());
        assert_eq!(damage.take(window(), false, 0), [window()]);
        assert!(!damage.is_pending(), "nothing is owed after a frame");
        assert!(damage.take(window(), false, 0).is_empty());
    }

    #[test]
    fn damage_is_grown_by_the_margin_it_is_taken_with() {
        let mut damage = Damage::default();
        damage.add(Rect::new(100, 100, 120, 110));
        assert_eq!(damage.rects(), [Rect::new(100, 100, 120, 110)]);
        assert_eq!(
            damage.take(window(), false, 8),
            [Rect::new(92, 92, 128, 118)],
            "the margin is the caller's, so it can follow the DPI"
        );
        damage.add(Rect::new(0, 0, 0, 0));
        assert!(damage.rects().is_empty(), "an empty rectangle is no damage");
        damage.add(Rect::new(100, 100, 120, 110));
        assert_eq!(
            damage.take(window(), false, 16),
            [Rect::new(84, 84, 136, 126)]
        );
    }

    #[test]
    fn a_rectangle_inside_another_adds_nothing() {
        let mut damage = Damage::default();
        damage.add(Rect::new(100, 100, 300, 300));
        damage.add(Rect::new(150, 150, 160, 160));
        assert_eq!(damage.rects().len(), 1);
    }

    #[test]
    fn many_tiny_adjacent_damages_merge_into_one_rectangle() {
        let mut damage = Damage::default();
        for cell in 0..200 {
            damage.add(Rect::new(10 + cell * 3, 40, 13 + cell * 3, 52));
        }
        assert_eq!(damage.rects().len(), 1, "a row of cells is one rectangle");
        let rect = damage.rects()[0];
        assert_eq!(rect.left, 10);
        assert_eq!(rect.right, 10 + 200 * 3);
    }

    #[test]
    fn two_distant_damages_stay_two_rectangles() {
        let mut damage = Damage::default();
        damage.add(Rect::new(10, 10, 40, 40));
        damage.add(Rect::new(900, 700, 940, 740));
        assert_eq!(damage.rects().len(), 2, "no window-sized union");
    }

    #[test]
    fn a_horizontal_and_a_vertical_rule_do_not_merge_into_their_big_union() {
        let mut damage = Damage::default();
        damage.add(Rect::new(0, 400, 1000, 401));
        damage.add(Rect::new(500, 0, 501, 800));
        assert_eq!(damage.rects().len(), 2);
    }

    #[test]
    fn the_list_never_grows_past_its_bound() {
        let mut damage = Damage::default();
        // Far apart, so none merge on their own.
        for step in 0..100 {
            let x = (step % 10) * 90;
            let y = (step / 10) * 75;
            damage.add(Rect::new(x, y, x + 4, y + 4));
            assert!(damage.rects().len() <= MAX_RECTS);
        }
        let held = damage.rects().to_vec();
        for step in 0..100 {
            let (x, y) = ((step % 10) * 90, (step / 10) * 75);
            assert!(
                held.iter()
                    .any(|rect| contains(*rect, Rect::new(x, y, x + 4, y + 4))),
                "merging only ever covers more: ({x}, {y})"
            );
        }
    }

    #[test]
    fn taking_trims_to_the_window_and_drops_what_is_outside() {
        let mut damage = Damage::default();
        damage.add(Rect::new(-50, 10, 30, 30));
        damage.add(Rect::new(2000, 2000, 2010, 2010));
        let taken = damage.take(window(), false, 0);
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].left, 0);
        assert!(contains(window(), taken[0]));
    }

    #[test]
    fn damage_that_covers_the_window_is_one_whole_window_repaint() {
        let mut damage = Damage::default();
        damage.add(Rect::new(0, 0, 1000, 500));
        damage.add(Rect::new(0, 500, 1000, 800));
        assert_eq!(damage.take(window(), false, 0), [window()]);
    }

    #[test]
    fn a_full_invalidate_swallows_the_list_and_later_damage() {
        let mut damage = Damage::default();
        damage.add(Rect::new(1, 1, 5, 5));
        damage.add_all();
        damage.add(Rect::new(10, 10, 20, 20));
        assert!(damage.is_full());
        assert_eq!(damage.take(window(), false, 0), [window()]);
        assert!(!damage.is_pending());
    }

    #[test]
    fn force_repaints_the_whole_window_even_with_a_small_list() {
        let mut damage = Damage::default();
        damage.add(Rect::new(1, 1, 5, 5));
        assert_eq!(damage.take(window(), true, 0), [window()]);
    }
}
