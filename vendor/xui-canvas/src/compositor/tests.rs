use std::cell::Cell;
use std::rc::Rc;

use super::*;
use crate::geometry::{absolute_bounds, ancestor_clip, effectively_visible};
use crate::scene::Node;
use xui_core::Color;

/// Content that never has any.
struct Nothing;

impl Content for Nothing {
    fn has_node(&self, _id: WidgetId) -> bool {
        false
    }

    fn is_live(&self) -> bool {
        false
    }

    fn paint_window(&self, _region: &mut Region<'_>, _window: Rect, _dpi: u32, _theme: &Theme) {}

    fn paint_node(
        &self,
        _region: &mut Region<'_>,
        _id: WidgetId,
        _bounds: Rect,
        _clip: Option<Rect>,
        _dpi: u32,
        _theme: &Theme,
    ) {
    }
}

fn window() -> WindowId {
    WindowId::from_raw(1)
}

fn painter(color: Color) -> Painter {
    Rc::new(move |canvas| {
        let bounds = canvas.bounds();
        canvas.fill_rect(bounds, color);
    })
}

/// A painter that counts how often it runs.
fn counting(color: Color, runs: &Rc<Cell<u32>>) -> Painter {
    let runs = Rc::clone(runs);
    Rc::new(move |canvas| {
        runs.set(runs.get() + 1);
        let bounds = canvas.bounds();
        canvas.fill_rect(bounds, color);
    })
}

fn node(parent: ParentRef, bounds: Rect) -> Node {
    Node::new(window(), parent, bounds)
}

fn top(bounds: Rect) -> Node {
    node(ParentRef::Window(window()), bounds)
}

fn frame(size: (i32, i32)) -> (Rect, Theme) {
    (Rect::new(0, 0, size.0, size.1), Theme::light())
}

fn paint_whole(scene: &Scene, surface: &mut Surface, size: (i32, i32)) {
    let (window_rect, theme) = frame(size);
    let plan = plan(scene, window(), 96, &Nothing);
    let frame = FrameInfo {
        window: window_rect,
        dpi: 96,
        theme: &theme,
    };
    plan.paint(surface, &[window_rect], &frame, &Nothing);
}

fn pixel(surface: &Surface, x: u32, y: u32) -> [u8; 4] {
    surface.to_image().pixel(x, y).expect("in bounds")
}

/// A small deterministic generator, so the random scenes repeat.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }

    fn range(&mut self, low: i32, high: i32) -> i32 {
        low + self.below((high - low) as u64) as i32
    }
}

/// A random forest of nodes: containers nested inside containers, some hidden,
/// some clipping their descendants, some painting, some raised.
fn random_scene(seed: u64) -> Scene {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut scene = Scene::new();
    let mut made: Vec<WidgetId> = Vec::new();
    for raw in 1..=rng.range(4, 40) as u64 {
        let id = WidgetId::from_raw(raw);
        let parent = if made.is_empty() || rng.below(4) == 0 {
            ParentRef::Window(window())
        } else {
            ParentRef::Widget(made[rng.below(made.len() as u64) as usize])
        };
        let left = rng.range(-20, 120);
        let top_edge = rng.range(-20, 90);
        let mut node = node(
            parent,
            Rect::new(
                left,
                top_edge,
                left + rng.range(0, 70),
                top_edge + rng.range(0, 50),
            ),
        );
        node.visible = rng.below(6) != 0;
        if rng.below(3) == 0 {
            let at = rng.range(-10, 40);
            node.clip = Some(Rect::new(
                at,
                at,
                at + rng.range(0, 60),
                at + rng.range(0, 60),
            ));
        }
        if rng.below(3) != 0 {
            node.painter = Some(painter(Color::rgb(raw as u8 * 6, 90, 200)));
        }
        scene.insert(id, node);
        made.push(id);
        if rng.below(5) == 0 {
            scene.raise(made[rng.below(made.len() as u64) as usize]);
        }
    }
    scene
}

/// What a frame's nodes would be from the per-node walks the compositor
/// replaced: visible through the ancestry, with something left in the clip.
#[test]
fn the_one_pass_plan_matches_the_per_node_walks_on_random_scenes() {
    for seed in 1..=300 {
        let scene = random_scene(seed);
        let planned = plan(&scene, window(), 96, &Nothing);
        let mut expected = Vec::new();
        for (id, node) in scene.iter() {
            if node.painter.is_none() || !effectively_visible(&scene, id) {
                continue;
            }
            let bounds = absolute_bounds(&scene, id).unwrap();
            let clip = ancestor_clip(&scene, id);
            if let Some(clip) = clip
                && intersect(bounds, clip).is_empty()
            {
                continue;
            }
            expected.push((id, bounds, clip));
        }
        let got: Vec<(WidgetId, Rect, Option<Rect>)> = planned
            .draws
            .iter()
            .map(|draw| (draw.id, draw.bounds, draw.clip))
            .collect();
        assert_eq!(got, expected, "seed {seed}");
    }
}

#[test]
fn nodes_paint_in_creation_order_and_a_raised_node_paints_last() {
    let mut scene = Scene::new();
    let (red, blue) = (WidgetId::from_raw(1), WidgetId::from_raw(2));
    let mut a = top(Rect::new(0, 0, 20, 20));
    a.painter = Some(painter(Color::rgb(255, 0, 0)));
    let mut b = top(Rect::new(10, 10, 30, 30));
    b.painter = Some(painter(Color::rgb(0, 0, 255)));
    scene.insert(red, a);
    scene.insert(blue, b);

    let mut surface = Surface::new(40, 40);
    paint_whole(&scene, &mut surface, (40, 40));
    assert_eq!(
        pixel(&surface, 15, 15),
        [0, 0, 255, 255],
        "the later node is on top"
    );

    scene.raise(red);
    paint_whole(&scene, &mut surface, (40, 40));
    assert_eq!(
        pixel(&surface, 15, 15),
        [255, 0, 0, 255],
        "raised: now on top"
    );
}

#[test]
fn an_area_repaints_only_the_nodes_whose_reach_meets_it() {
    let mut scene = Scene::new();
    let near = Rc::new(Cell::new(0));
    let within_margin = Rc::new(Cell::new(0));
    let far = Rc::new(Cell::new(0));
    let mut a = top(Rect::new(0, 0, 20, 20));
    a.painter = Some(counting(Color::rgb(255, 0, 0), &near));
    // 4 px right of the area: its overdraw can reach in.
    let mut b = top(Rect::new(54, 0, 80, 20));
    b.painter = Some(counting(Color::rgb(0, 255, 0), &within_margin));
    // Further than the margin: it cannot.
    let mut c = top(Rect::new(80, 0, 120, 20));
    c.painter = Some(counting(Color::rgb(0, 0, 255), &far));
    scene.insert(WidgetId::from_raw(1), a);
    scene.insert(WidgetId::from_raw(2), b);
    scene.insert(WidgetId::from_raw(3), c);

    let mut surface = Surface::new(200, 40);
    let theme = Theme::light();
    let frame = FrameInfo {
        window: Rect::new(0, 0, 200, 40),
        dpi: 96,
        theme: &theme,
    };
    let planned = plan(&scene, window(), 96, &Nothing);
    planned.paint(&mut surface, &[Rect::new(0, 0, 50, 20)], &frame, &Nothing);

    assert_eq!(near.get(), 1, "the node inside the area paints");
    assert_eq!(
        within_margin.get(),
        1,
        "a node within the overdraw margin may draw into the area"
    );
    assert_eq!(far.get(), 0, "a node beyond the margin is skipped");
}

#[test]
fn painting_an_area_leaves_every_other_pixel_alone() {
    let mut scene = Scene::new();
    let mut a = top(Rect::new(0, 0, 100, 30));
    a.painter = Some(painter(Color::rgb(255, 0, 0)));
    scene.insert(WidgetId::from_raw(1), a);

    let mut surface = Surface::new(100, 30);
    surface.fill(Color::rgb(7, 8, 9));
    let theme = Theme::light();
    let frame = FrameInfo {
        window: Rect::new(0, 0, 100, 30),
        dpi: 96,
        theme: &theme,
    };
    plan(&scene, window(), 96, &Nothing).paint(
        &mut surface,
        &[Rect::new(10, 5, 20, 15)],
        &frame,
        &Nothing,
    );
    assert_eq!(pixel(&surface, 15, 10), [255, 0, 0, 255]);
    assert_eq!(pixel(&surface, 9, 10), [7, 8, 9, 255]);
    assert_eq!(pixel(&surface, 20, 10), [7, 8, 9, 255]);
    assert_eq!(pixel(&surface, 15, 15), [7, 8, 9, 255]);
}

#[test]
fn a_hidden_ancestor_and_a_clip_remove_a_node_from_the_plan() {
    let mut scene = Scene::new();
    let (panel, shown, hidden, clipped) = (
        WidgetId::from_raw(1),
        WidgetId::from_raw(2),
        WidgetId::from_raw(3),
        WidgetId::from_raw(4),
    );
    let mut p = top(Rect::new(0, 0, 100, 100));
    p.clip = Some(Rect::new(0, 0, 100, 10));
    scene.insert(panel, p);
    let mut a = node(ParentRef::Widget(panel), Rect::new(0, 0, 50, 8));
    a.painter = Some(painter(Color::rgb(1, 2, 3)));
    let mut b = node(ParentRef::Widget(panel), Rect::new(0, 0, 50, 8));
    b.painter = Some(painter(Color::rgb(1, 2, 3)));
    b.visible = false;
    let mut c = node(ParentRef::Widget(panel), Rect::new(0, 40, 50, 60));
    c.painter = Some(painter(Color::rgb(1, 2, 3)));
    scene.insert(shown, a);
    scene.insert(hidden, b);
    scene.insert(clipped, c);

    let planned = plan(&scene, window(), 96, &Nothing);
    assert_eq!(planned.ids(), [shown]);
    assert_eq!(planned.len(), 1);
}

#[test]
fn a_node_with_no_area_still_paints_where_its_corner_is_repainted() {
    // A painter may draw around a node that has no area (a caret, a ring).
    let mut scene = Scene::new();
    let runs = Rc::new(Cell::new(0));
    let mut empty = top(Rect::new(30, 30, 30, 30));
    empty.painter = Some(counting(Color::rgb(1, 2, 3), &runs));
    scene.insert(WidgetId::from_raw(1), empty);

    let mut surface = Surface::new(100, 100);
    let theme = Theme::light();
    let info = FrameInfo {
        window: Rect::new(0, 0, 100, 100),
        dpi: 96,
        theme: &theme,
    };
    let planned = plan(&scene, window(), 96, &Nothing);
    planned.paint(&mut surface, &[Rect::new(26, 26, 40, 40)], &info, &Nothing);
    assert_eq!(runs.get(), 1, "its place is inside the repainted area");
    planned.paint(&mut surface, &[Rect::new(60, 60, 90, 90)], &info, &Nothing);
    assert_eq!(runs.get(), 1, "and not where it cannot reach");
}

#[test]
fn growing_a_rectangle_at_the_limits_stays_representable() {
    let huge = Rect::new(0, 0, i32::MAX, i32::MAX);
    let grown = grow(huge, margin(96));
    assert!(!grown.is_empty());
    assert!(grown.width() > 0 && grown.height() > 0, "no overflow");
    assert!(grow(Rect::new(0, 0, 0, 0), 5).is_empty());
}
