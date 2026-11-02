//! Where blocks are on the page, in pixels, and what is under a point. Block
//! positions may come from a hostile file, up to 65535 by 65535, which at any DPI
//! here still fits an `i32`: 2 * 65535 * 1024.

use mscent_texgen::recipe::Block;
use xui::geometry::Rect;

/// A column's width and a row's height at 96 DPI. A column is wide enough for the
/// longest operator name beside a thumbnail.
const CELL: (i32, i32) = (144, 56);
/// Width of the part of a block's right edge that resizes it at 96 DPI.
const EDGE: i32 = 8;
/// Largest DPI scale counted on: 4x.
const MAX_DPI: u32 = 384;

/// The grid at one DPI and scroll position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub cell: (i32, i32),
    pub edge: i32,
    pub scroll: (i32, i32),
}

/// What is under a point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub block: usize,
    pub on_right_edge: bool,
}

impl Grid {
    pub fn new(dpi: u32, scroll: (i32, i32)) -> Self {
        let scale = |v: i32| (v * dpi.clamp(48, MAX_DPI) as i32 / 96).max(1);
        Self {
            cell: (scale(CELL.0), scale(CELL.1)),
            edge: scale(EDGE),
            scroll,
        }
    }

    /// The rectangle of a block, in the node's pixels.
    pub fn rect(&self, b: &Block) -> Rect {
        let left = i32::from(b.x) * self.cell.0 - self.scroll.0;
        let top = i32::from(b.y) * self.cell.1 - self.scroll.1;
        Rect::new(
            left,
            top,
            left + i32::from(b.w) * self.cell.0,
            top + self.cell.1,
        )
    }

    /// The cell under a point; may be past the page or negative, for the caller to
    /// clamp.
    pub fn cell_at(&self, x: i32, y: i32) -> (i32, i32) {
        (
            (x + self.scroll.0).div_euclid(self.cell.0),
            (y + self.scroll.1).div_euclid(self.cell.1),
        )
    }

    /// The block under a point. Blocks may overlap on a broken page; the last one
    /// drawn is the one on top, so it is the one found.
    pub fn hit(&self, blocks: &[Block], x: i32, y: i32) -> Option<Hit> {
        let block = blocks.iter().rposition(|b| {
            let r = self.rect(b);
            x >= r.left && x < r.right && y >= r.top && y < r.bottom
        })?;
        let on_right_edge = x >= self.rect(&blocks[block]).right - self.edge;
        Some(Hit {
            block,
            on_right_edge,
        })
    }

    /// The width a block `b` gets with its right edge dragged to `x`.
    pub fn width_at(&self, b: &Block, x: i32) -> u16 {
        let columns = (x + self.scroll.0 - i32::from(b.x) * self.cell.0 + self.cell.0 / 2)
            .div_euclid(self.cell.0);
        columns.clamp(1, i32::from(u16::MAX)) as u16
    }

    /// The size of the page: past the last block by a column and a row, which is
    /// room to add more.
    pub fn content(&self, blocks: &[Block]) -> (i32, i32) {
        let right = blocks
            .iter()
            .map(|b| i32::from(b.x) + i32::from(b.w))
            .max()
            .unwrap_or(0);
        let bottom = blocks.iter().map(|b| i32::from(b.y) + 1).max().unwrap_or(0);
        (
            (right.min(64) + 1) * self.cell.0,
            (bottom.min(256) + 1) * self.cell.1,
        )
    }

    /// The furthest the page scrolls when `viewport` of it shows: until its end
    /// is at the window's.
    pub fn scroll_limit(&self, blocks: &[Block], viewport: (i32, i32)) -> (i32, i32) {
        let content = self.content(blocks);
        (
            (content.0 - viewport.0).max(0),
            (content.1 - viewport.1).max(0),
        )
    }

    /// The scroll that brings a block into `viewport`: the same one if it is in
    /// view, else the least change that shows it (its top left, if it is bigger
    /// than the window).
    pub fn revealing(&self, block: &Block, viewport: (i32, i32)) -> (i32, i32) {
        let r = self.rect(block);
        let fit = |low: i32, high: i32, size: i32, scroll: i32| {
            if low < 0 || high - low > size {
                scroll + low
            } else if high > size {
                scroll + high - size
            } else {
                scroll
            }
        };
        (
            fit(r.left, r.right, viewport.0, self.scroll.0),
            fit(r.top, r.bottom, viewport.1, self.scroll.1),
        )
    }

    /// Where on a block's top edge to mark each of the blocks it reads, as x in the
    /// node's pixels. A block above it is marked where they overlap; one read by
    /// name (a `load`) is anywhere, so it is marked in the middle.
    pub fn mark_xs(&self, blocks: &[Block], block: usize, sources: &[usize]) -> Vec<i32> {
        let Some(me) = blocks.get(block).map(|b| self.rect(b)) else {
            return Vec::new();
        };
        sources
            .iter()
            .filter_map(|&s| blocks.get(s))
            .map(|s| {
                let from = self.rect(s);
                // Read from the row above where the two overlap; a `load` that
                // happens to sit under its store, but in another column, reads it
                // by name all the same.
                let overlap = (from.left.max(me.left), from.right.min(me.right));
                let (left, right) = if from.bottom == me.top && overlap.0 < overlap.1 {
                    overlap
                } else {
                    (me.left, me.right)
                };
                ((left + right) / 2).clamp(me.left + self.edge, me.right - self.edge)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use mscent_texgen::ops::{self, Params};

    use super::*;

    fn block(x: u16, y: u16, w: u16) -> Block {
        let spec = ops::find("flat").expect("flat");
        Block {
            x,
            y,
            w,
            spec,
            params: Params::defaults(spec),
        }
    }

    fn grid() -> Grid {
        Grid::new(96, (0, 0))
    }

    #[test]
    fn a_point_finds_its_block_and_the_right_edge_is_its_own_part() {
        let blocks = [block(1, 2, 2)];
        let g = grid();
        let r = g.rect(&blocks[0]);
        assert_eq!(r, Rect::new(144, 112, 432, 168));
        assert_eq!(g.hit(&blocks, 143, 120), None);
        assert_eq!(g.hit(&blocks, 144, 111), None);
        assert_eq!(
            g.hit(&blocks, 150, 120),
            Some(Hit {
                block: 0,
                on_right_edge: false
            })
        );
        assert_eq!(
            g.hit(&blocks, 428, 120),
            Some(Hit {
                block: 0,
                on_right_edge: true
            })
        );
        assert_eq!(g.hit(&blocks, 432, 120), None);
    }

    #[test]
    fn overlapping_blocks_find_the_one_on_top() {
        let blocks = [block(0, 0, 1), block(0, 0, 1)];
        assert_eq!(grid().hit(&blocks, 10, 10).expect("a block").block, 1);
    }

    #[test]
    fn scrolling_moves_the_page_under_the_pointer() {
        let g = Grid::new(96, (144, 56));
        assert_eq!(g.cell_at(0, 0), (1, 1));
        assert_eq!(g.rect(&block(1, 1, 1)).left, 0);
        assert_eq!(Grid::new(96, (0, 0)).cell_at(-1, -1), (-1, -1));
    }

    #[test]
    fn dragging_the_edge_rounds_to_a_column_and_keeps_one() {
        let g = grid();
        let b = block(1, 0, 1);
        assert_eq!(g.width_at(&b, 144 + 144), 1);
        assert_eq!(g.width_at(&b, 144 + 144 + 80), 2);
        assert_eq!(g.width_at(&b, 0), 1);
        assert_eq!(g.width_at(&b, i32::MAX / 2), u16::MAX);
    }

    #[test]
    fn hostile_positions_stay_in_range_at_every_dpi() {
        for dpi in [0, 96, 384, u32::MAX] {
            let g = Grid::new(dpi, (0, 0));
            let far = block(u16::MAX, u16::MAX, u16::MAX);
            let r = g.rect(&far);
            assert!(r.right > r.left && r.bottom > r.top, "{dpi}");
            assert!(g.hit(&[far], 0, 0).is_none());
        }
        assert_eq!(grid().scroll_limit(&[], (200, 100)), (0, 0));
        let far = [block(u16::MAX, u16::MAX, u16::MAX)];
        assert_eq!(
            grid().scroll_limit(&far, (100, 100)),
            (65 * 144 - 100, 257 * 56 - 100)
        );
    }

    #[test]
    fn a_block_out_of_view_is_scrolled_in_by_the_least_and_one_in_view_stays() {
        let view = (400, 224);
        let at = |scroll| Grid::new(96, scroll);
        let low = block(0, 10, 1);
        assert_eq!(at((0, 0)).revealing(&low, view), (0, 560 + 56 - 224));
        assert_eq!(at((0, 400)).revealing(&low, view), (0, 400), "in view");
        assert_eq!(at((0, 900)).revealing(&low, view), (0, 560), "above it");
        let far = block(5, 0, 1);
        assert_eq!(at((0, 0)).revealing(&far, view).0, 5 * 144 + 144 - 400);
        let wide = block(0, 3, 9);
        assert_eq!(at((100, 0)).revealing(&wide, view).0, 0, "wider than it");
    }

    #[test]
    fn the_page_is_a_column_and_a_row_longer_than_its_blocks() {
        let g = grid();
        assert_eq!(g.content(&[]), (144, 56));
        assert_eq!(g.content(&[block(1, 2, 3)]), (5 * 144, 4 * 56));
    }

    #[test]
    fn a_mark_is_where_the_block_above_meets_this_one_and_inside_its_edges() {
        let g = grid();
        // Two blocks above a three-wide one, in its first and third columns.
        let blocks = [
            block(0, 0, 1),
            block(2, 0, 1),
            block(0, 1, 3),
            block(5, 4, 1),
        ];
        assert_eq!(g.mark_xs(&blocks, 2, &[0, 1]), [72, 2 * 144 + 72]);
        // Read by name from far away: the middle of the block.
        assert_eq!(g.mark_xs(&blocks, 3, &[0]), [5 * 144 + 72]);
        // A block wider than the one it feeds is marked where they overlap.
        let wide = [block(0, 0, 4), block(2, 1, 1)];
        assert_eq!(g.mark_xs(&wide, 1, &[0]), [2 * 144 + 72]);
        // Directly below a block it does not overlap: still the middle.
        let beside = [block(0, 0, 1), block(2, 1, 1)];
        assert_eq!(g.mark_xs(&beside, 1, &[0]), [2 * 144 + 72]);
        assert!(g.mark_xs(&blocks, 9, &[0]).is_empty());
        assert!(g.mark_xs(&blocks, 2, &[9]).is_empty());
    }
}
