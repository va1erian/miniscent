//! Where blocks are on the page, in pixels, and what is under a point. Block
//! positions may come from a hostile file, up to 65535 by 65535, which at any DPI
//! here still fits an `i32`: 2 * 65535 * 1024.

use mscent_texgen::recipe::Block;
use xui::geometry::Rect;

/// A column's width and a row's height at 96 DPI.
const CELL: (i32, i32) = (128, 56);
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

    /// The furthest the page scrolls: past the last block, by room to add more.
    pub fn scroll_limit(&self, blocks: &[Block]) -> (i32, i32) {
        let right = blocks
            .iter()
            .map(|b| i32::from(b.x) + i32::from(b.w))
            .max()
            .unwrap_or(0);
        let bottom = blocks.iter().map(|b| i32::from(b.y) + 1).max().unwrap_or(0);
        (right.min(64) * self.cell.0, bottom.min(256) * self.cell.1)
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
        assert_eq!(r, Rect::new(128, 112, 384, 168));
        assert_eq!(g.hit(&blocks, 127, 120), None);
        assert_eq!(g.hit(&blocks, 128, 111), None);
        assert_eq!(
            g.hit(&blocks, 140, 120),
            Some(Hit {
                block: 0,
                on_right_edge: false
            })
        );
        assert_eq!(
            g.hit(&blocks, 380, 120),
            Some(Hit {
                block: 0,
                on_right_edge: true
            })
        );
        assert_eq!(g.hit(&blocks, 384, 120), None);
    }

    #[test]
    fn overlapping_blocks_find_the_one_on_top() {
        let blocks = [block(0, 0, 1), block(0, 0, 1)];
        assert_eq!(grid().hit(&blocks, 10, 10).expect("a block").block, 1);
    }

    #[test]
    fn scrolling_moves_the_page_under_the_pointer() {
        let g = Grid::new(96, (128, 56));
        assert_eq!(g.cell_at(0, 0), (1, 1));
        assert_eq!(g.rect(&block(1, 1, 1)).left, 0);
        assert_eq!(Grid::new(96, (0, 0)).cell_at(-1, -1), (-1, -1));
    }

    #[test]
    fn dragging_the_edge_rounds_to_a_column_and_keeps_one() {
        let g = grid();
        let b = block(1, 0, 1);
        assert_eq!(g.width_at(&b, 128 + 128), 1);
        assert_eq!(g.width_at(&b, 128 + 128 + 70), 2);
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
        assert_eq!(grid().scroll_limit(&[]), (0, 0));
        let far = [block(u16::MAX, u16::MAX, u16::MAX)];
        assert_eq!(grid().scroll_limit(&far), (64 * 128, 256 * 56));
    }
}
