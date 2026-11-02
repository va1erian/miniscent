//! The page as it looks and as it scrolls, on real widgets: the marks for what a
//! block reads are looked for in the pixels, and the stock recipes, which are
//! longer than the page is tall, are walked to their last block.

use xui::message::{Modifiers, MouseButton};
use xui::xui_canvas::snapshot::Stage;
use xui::{Image, backend::Event};

use super::TexMsg;
use super::page_geom::Grid;
use super::tests::{Handles, mouse_event, open, steady_shot, with_workspace};
use crate::ui::Msg;
use mscent_texgen::ops;

fn add(stage: &Stage<'_, Msg>, name: &str, cell: (u16, u16)) {
    let op = ops::all().position(|o| o.name == name).expect(name);
    stage.emit(Msg::Tex(TexMsg::Add { op, cell }));
}

/// The window's pixel at a point of the page's node.
fn at(img: &Image, h: &Handles, (x, y): (i32, i32)) -> [u8; 4] {
    let (ox, oy) = (h.origin)();
    img.pixel((ox + x) as u32, (oy + y) as u32).expect("inside")
}

/// Whether the top edge of a block has a mark where its inputs are read, by
/// comparing the pixel there with one beside it on the block's plain fill.
fn marks_of(stage: &Stage<'_, Msg>, h: &Handles, block: usize) -> Vec<bool> {
    let img = steady_shot(stage);
    let studio = h.studio.borrow();
    let doc = studio.doc().expect("open");
    let blocks = &doc.recipe().blocks;
    let grid = Grid::new(stage.ui().dpi(), (0, 0));
    let top = grid.rect(&blocks[block]).top + 3;
    let sources = doc.page().as_ref().map_or(&[][..], |g| g.inputs_of(block));
    let plain = at(&img, h, (grid.rect(&blocks[block]).left + 14, top));
    grid.mark_xs(blocks, block, sources)
        .into_iter()
        .map(|x| at(&img, h, (x, top)) != plain)
        .collect()
}

#[test]
fn a_shape_below_another_block_shows_a_mark_where_it_reads_and_one_alone_shows_none() {
    with_workspace(|stage, h| {
        // The starter page has a flat over the albedo output in column 0.
        add(stage, "noise", (2, 0));
        add(stage, "rect", (2, 1));
        let below = h
            .studio
            .borrow()
            .doc()
            .expect("open")
            .selected()
            .expect("it");
        add(stage, "rect", (4, 0));
        let alone = h
            .studio
            .borrow()
            .doc()
            .expect("open")
            .selected()
            .expect("it");
        {
            let studio = h.studio.borrow();
            let graph = studio
                .doc()
                .expect("open")
                .page()
                .as_ref()
                .expect("compiles");
            assert_eq!(graph.inputs_of(below).len(), 1, "it reads the noise");
            assert!(graph.inputs_of(alone).is_empty(), "this one reads nothing");
        }
        assert_eq!(
            marks_of(stage, h, below),
            [true],
            "a mark for its one input"
        );
        assert!(marks_of(stage, h, alone).is_empty());
        // The pixels at the middle of the lonely one's top edge are plain.
        let img = steady_shot(stage);
        let grid = Grid::new(stage.ui().dpi(), (0, 0));
        let studio = h.studio.borrow();
        let rect = grid.rect(&studio.doc().expect("open").recipe().blocks[alone]);
        let (mid, edge) = (
            at(&img, h, ((rect.left + rect.right) / 2, rect.top + 3)),
            at(&img, h, (rect.left + 14, rect.top + 3)),
        );
        assert_eq!(mid, edge, "nothing is drawn where nothing is read");
    });
}

#[test]
fn a_merge_shows_a_mark_for_each_of_its_inputs() {
    with_workspace(|stage, h| {
        add(stage, "noise", (3, 0));
        add(stage, "flat", (4, 0));
        add(stage, "merge", (3, 1));
        let merge = h
            .studio
            .borrow()
            .doc()
            .expect("open")
            .selected()
            .expect("it");
        assert_eq!(marks_of(stage, h, merge), [true, true]);
    });
}

fn wheel(stage: &Stage<'_, Msg>, h: &Handles, notches: i16) {
    let (ox, oy) = (h.origin)();
    stage.inject(Event::MouseWheel {
        delta: notches,
        horizontal: false,
        x: ox + 100,
        y: oy + 100,
        modifiers: Modifiers::NONE,
    });
}

fn block_of(h: &Handles, index: usize) -> mscent_texgen::recipe::Block {
    h.studio.borrow().doc().expect("open").recipe().blocks[index].clone()
}

#[test]
fn the_wheel_reaches_the_last_block_of_a_stock_recipe_and_back() {
    with_workspace(|stage, h| {
        open(stage, h, "brick");
        let _ = steady_shot(stage);
        let count = h.studio.borrow().doc().expect("open").recipe().blocks.len();
        assert!(count >= 30, "brick is a recipe of {count} blocks");
        let (first, last) = (block_of(h, 0), block_of(h, count - 1));
        assert!(h.page.shows(&first) && !h.page.shows(&last));
        for _ in 0..60 {
            wheel(stage, h, -1);
        }
        assert!(h.page.shows(&last), "the last block is in view at the end");
        assert!(!h.page.shows(&first));
        for _ in 0..60 {
            wheel(stage, h, 1);
        }
        assert!(h.page.shows(&first) && !h.page.shows(&last));
    });
}

#[test]
fn selecting_any_block_of_any_stock_recipe_brings_it_into_view() {
    with_workspace(|stage, h| {
        for name in ["brick", "floor", "lightpanel", "metal", "rock"] {
            open(stage, h, name);
            let _ = steady_shot(stage);
            let count = h.studio.borrow().doc().expect("open").recipe().blocks.len();
            assert!(count > 1, "{name} opened as a recipe, not a blank page");
            // Down the page and back up, so that both directions are scrolled.
            let order = (0..count).chain((0..count).rev());
            for i in order {
                stage.emit(Msg::Tex(TexMsg::Select(Some(i))));
                assert!(
                    h.page.shows(&block_of(h, i)),
                    "{name}: block {i} is out of view"
                );
            }
        }
    });
}

#[test]
fn a_click_on_the_page_still_selects_what_is_under_the_pointer_after_scrolling() {
    with_workspace(|stage, h| {
        open(stage, h, "brick");
        let _ = steady_shot(stage);
        // Four notches of the wheel, a row each.
        for _ in 0..4 {
            wheel(stage, h, -1);
        }
        assert_eq!(h.page.scroll().1, 4 * 56, "a notch is a row");
        let grid = Grid::new(stage.ui().dpi(), h.page.scroll());
        let target = block_of(h, 8);
        let rect = grid.rect(&target);
        let (ox, oy) = (h.origin)();
        let (x, y) = (ox + rect.left + 40, oy + rect.top + 20);
        stage.inject(mouse_event(0, x, y, MouseButton::Left));
        stage.inject(mouse_event(2, x, y, MouseButton::Left));
        assert_eq!(h.studio.borrow().doc().expect("open").selected(), Some(8));
    });
}
