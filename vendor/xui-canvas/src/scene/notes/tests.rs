use std::rc::Rc;

use xui_core::backend::{Painter, ParentRef, WidgetId, WindowId};

use super::super::Node;
use super::*;
use crate::compositor::grow;

/// The overdraw margin at 96 DPI.
const MARGIN: i32 = 8;

fn window() -> WindowId {
    WindowId::from_raw(1)
}

fn painter() -> Painter {
    Rc::new(|_| {})
}

const WINDOW: Rect = Rect::new(0, 0, 1000, 800);

/// A scene whose first frame was taken, so what follows is incremental.
fn settled() -> Scene {
    let mut scene = Scene::new();
    scene.invalidate_window(window());
    scene.take_damage(window(), WINDOW, false, MARGIN);
    scene
}

fn painting(scene: &mut Scene, id: u64, parent: ParentRef, bounds: Rect) -> WidgetId {
    let id = WidgetId::from_raw(id);
    let mut node = Node::new(window(), parent, bounds);
    node.painter = Some(painter());
    scene.insert(id, node);
    id
}

fn top() -> ParentRef {
    ParentRef::Window(window())
}

fn taken(scene: &mut Scene) -> Vec<Rect> {
    scene.take_damage(window(), WINDOW, false, MARGIN)
}

fn grown(rect: Rect) -> Rect {
    grow(rect, MARGIN)
}

#[test]
fn a_first_frame_is_everything() {
    let mut scene = Scene::new();
    assert_eq!(scene.take_damage(window(), WINDOW, false, MARGIN), [WINDOW]);
    assert!(taken(&mut scene).is_empty(), "then nothing is owed");
}

#[test]
fn moving_a_node_marks_its_old_and_new_places() {
    let mut scene = settled();
    let id = painting(&mut scene, 1, top(), Rect::new(100, 100, 150, 130));
    scene.set_bounds(id, Rect::new(400, 300, 450, 330));
    let damage = taken(&mut scene);
    assert!(
        damage
            .iter()
            .any(|rect| *rect == grown(Rect::new(400, 300, 450, 330))),
        "the new place: {damage:?}"
    );
    // The node had no painter-visible place before its first move except the
    // one it was created at, which the move marks too.
    assert!(
        damage
            .iter()
            .any(|rect| rect.left <= 100 - MARGIN && rect.right >= 150 + MARGIN),
        "the old place: {damage:?}"
    );
}

#[test]
fn a_move_to_the_same_place_marks_nothing() {
    let mut scene = settled();
    let id = painting(&mut scene, 1, top(), Rect::new(100, 100, 150, 130));
    taken(&mut scene);
    scene.set_bounds(id, Rect::new(100, 100, 150, 130));
    assert!(taken(&mut scene).is_empty());
}

#[test]
fn moving_a_container_marks_everything_inside_it() {
    let mut scene = settled();
    let panel = painting(&mut scene, 1, top(), Rect::new(10, 10, 60, 60));
    painting(
        &mut scene,
        2,
        ParentRef::Widget(panel),
        Rect::new(100, 100, 130, 120),
    );
    taken(&mut scene);
    scene.set_bounds(panel, Rect::new(10, 10, 70, 60).offset(200, 0));
    let damage = taken(&mut scene);
    // The child sat at (110, 110)..(140, 130) before and (310, 110)..(340, 130)
    // after; both are inside the damage.
    for place in [Rect::new(110, 110, 140, 130), Rect::new(310, 110, 340, 130)] {
        assert!(
            damage.iter().any(|rect| rect.left <= place.left
                && rect.top <= place.top
                && rect.right >= place.right
                && rect.bottom >= place.bottom),
            "{place:?} in {damage:?}"
        );
    }
}

#[test]
fn hiding_marks_the_old_place_and_a_hidden_node_marks_nothing() {
    let mut scene = settled();
    let id = painting(&mut scene, 1, top(), Rect::new(100, 100, 150, 130));
    taken(&mut scene);
    assert!(scene.set_visible(id, false));
    assert_eq!(taken(&mut scene), [grown(Rect::new(100, 100, 150, 130))]);
    assert!(!scene.set_visible(id, false), "already hidden");
    // Moving or invalidating a hidden node changes nothing on screen.
    scene.set_bounds(id, Rect::new(300, 300, 350, 330));
    scene.invalidate(id);
    assert!(taken(&mut scene).is_empty());
}

#[test]
fn destroying_marks_the_place_of_the_whole_subtree() {
    let mut scene = settled();
    let panel = painting(&mut scene, 1, top(), Rect::new(10, 10, 60, 60));
    painting(
        &mut scene,
        2,
        ParentRef::Widget(panel),
        Rect::new(100, 100, 130, 120),
    );
    taken(&mut scene);
    scene.remove(panel);
    let damage = taken(&mut scene);
    assert!(
        damage.iter().any(|rect| rect.right >= 140 + MARGIN),
        "the child's place, far outside its parent: {damage:?}"
    );
    assert!(scene.get(panel).is_none());
}

#[test]
fn a_clip_marks_what_it_hides_and_shows() {
    let mut scene = settled();
    let view = painting(&mut scene, 1, top(), Rect::new(0, 0, 200, 200));
    painting(
        &mut scene,
        2,
        ParentRef::Widget(view),
        Rect::new(0, 100, 100, 150),
    );
    taken(&mut scene);
    // A clip that shows only the top 50 rows hides the child entirely.
    assert!(scene.set_clip(view, Some(Rect::new(0, 0, 200, 50))));
    let damage = taken(&mut scene);
    assert!(
        damage
            .iter()
            .any(|rect| rect.top <= 100 && rect.bottom >= 150),
        "the child that vanished: {damage:?}"
    );
    assert!(
        !scene.set_clip(view, Some(Rect::new(0, 0, 200, 50))),
        "the same clip"
    );
}

#[test]
fn raising_marks_the_node_and_a_raise_of_the_top_node_marks_nothing() {
    let mut scene = settled();
    let below = painting(&mut scene, 1, top(), Rect::new(30, 30, 80, 80));
    let above = painting(&mut scene, 2, top(), Rect::new(50, 50, 100, 100));
    taken(&mut scene);
    assert!(!scene.raise(above), "already on top");
    assert!(taken(&mut scene).is_empty());
    assert!(scene.raise(below));
    assert_eq!(taken(&mut scene), [grown(Rect::new(30, 30, 80, 80))]);
}

#[test]
fn invalidate_rect_is_in_the_nodes_coordinates_and_trimmed_by_ancestor_clips() {
    let mut scene = settled();
    let view = painting(&mut scene, 1, top(), Rect::new(100, 100, 300, 300));
    let child = painting(
        &mut scene,
        2,
        ParentRef::Widget(view),
        Rect::new(10, 10, 190, 190),
    );
    scene.set_clip(view, Some(Rect::new(0, 0, 100, 100)));
    taken(&mut scene);
    // (5, 5)..(25, 15) in the child is (115, 115)..(135, 125) in the window.
    scene.invalidate_rect(child, Rect::new(5, 5, 25, 15));
    assert_eq!(taken(&mut scene), [grown(Rect::new(115, 115, 135, 125))]);
    // Past the clip's edge (100 + 100 = 200) nothing of it is visible.
    scene.invalidate_rect(child, Rect::new(150, 150, 170, 170));
    assert!(taken(&mut scene).is_empty());
}

#[test]
fn text_and_painter_changes_mark_the_nodes_own_place() {
    let mut scene = settled();
    let id = painting(&mut scene, 1, top(), Rect::new(40, 40, 90, 60));
    taken(&mut scene);
    assert!(scene.set_text(id, "hello"));
    assert_eq!(taken(&mut scene), [grown(Rect::new(40, 40, 90, 60))]);
    assert!(!scene.set_text(id, "hello"), "the same text");
    assert!(taken(&mut scene).is_empty());
    assert!(scene.set_painter(id, painter()));
    assert_eq!(taken(&mut scene), [grown(Rect::new(40, 40, 90, 60))]);
}

#[test]
fn a_node_without_a_painter_marks_nothing_of_its_own() {
    let mut scene = settled();
    let id = WidgetId::from_raw(1);
    scene.insert(id, Node::new(window(), top(), Rect::new(0, 0, 500, 500)));
    taken(&mut scene);
    scene.set_bounds(id, Rect::new(10, 10, 510, 510));
    assert!(taken(&mut scene).is_empty(), "it paints nothing");
    assert!(
        scene.invalidate(id),
        "but an explicit invalidate is honoured"
    );
    assert_eq!(taken(&mut scene), [grown(Rect::new(10, 10, 510, 510))]);
}

#[test]
fn closing_a_window_forgets_its_damage() {
    let mut scene = settled();
    painting(&mut scene, 1, top(), Rect::new(0, 0, 10, 10));
    scene.invalidate(WidgetId::from_raw(1));
    scene.remove_window(window());
    // A window that comes back (a new one with the same id never does, but a
    // fresh scene entry must not inherit it) starts stale.
    assert_eq!(scene.take_damage(window(), WINDOW, false, MARGIN), [WINDOW]);
}

#[test]
fn a_node_with_no_area_marks_the_place_of_its_corner() {
    let mut scene = settled();
    let id = painting(&mut scene, 1, top(), Rect::new(30, 30, 30, 30));
    taken(&mut scene);
    scene.set_painter(id, painter());
    assert_eq!(taken(&mut scene), [grown(Rect::new(30, 30, 31, 31))]);
}

#[test]
fn every_input_and_focus_event_marks_its_target_but_a_pointer_move() {
    use xui_core::backend::Event;
    use xui_core::message::{Key, Modifiers, MouseButton};

    let press = Event::MouseDown {
        x: 1,
        y: 1,
        button: MouseButton::Left,
        modifiers: Modifiers::NONE,
    };
    let key = Event::KeyDown {
        key: Key::A,
        modifiers: Modifiers::NONE,
        repeat: 1,
        system: false,
    };
    for event in [
        &press,
        &key,
        &Event::Char('a'),
        &Event::SetFocus,
        &Event::KillFocus,
        &Event::MouseLeave,
        &Event::CaptureChanged,
    ] {
        assert!(event_marks_target(event), "{event:?}");
    }
    // The commonest event marks nothing: a hover arrives as the widget's own
    // invalidate and as the `MouseLeave` of the node the pointer left.
    let motion = Event::MouseMove {
        x: 1,
        y: 1,
        modifiers: Modifiers::NONE,
    };
    assert!(!event_marks_target(&motion));
}
