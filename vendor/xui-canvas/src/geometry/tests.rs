use super::*;

fn node(parent: ParentRef, bounds: Rect) -> Node {
    Node::new(WindowId::from_raw(1), parent, bounds)
}

fn window() -> ParentRef {
    ParentRef::Window(WindowId::from_raw(1))
}

/// A panel at (100, 50) with a child at (5, 7) inside it.
fn nested() -> (Scene, WidgetId, WidgetId) {
    let (panel, child) = (WidgetId::from_raw(10), WidgetId::from_raw(11));
    let mut scene = Scene::new();
    scene.insert(panel, node(window(), Rect::new(100, 50, 200, 90)));
    scene.insert(
        child,
        node(ParentRef::Widget(panel), Rect::new(5, 7, 25, 17)),
    );
    (scene, panel, child)
}

#[test]
fn nested_offsets_accumulate_to_window_coordinates() {
    let (scene, panel, child) = nested();
    assert_eq!(
        absolute_bounds(&scene, panel),
        Some(Rect::new(100, 50, 200, 90))
    );
    assert_eq!(
        absolute_bounds(&scene, child),
        Some(Rect::new(105, 57, 125, 67))
    );
}

#[test]
fn visibility_cascades_through_ancestors() {
    let (mut scene, panel, child) = nested();
    scene.get_mut(panel).unwrap().visible = false;
    assert!(
        !effectively_visible(&scene, child),
        "the hidden panel hides its child"
    );
}

#[test]
fn enabled_cascades_through_ancestors() {
    let (mut scene, panel, child) = nested();
    scene.get_mut(panel).unwrap().enabled = false;
    assert!(
        effectively_visible(&scene, child),
        "visibility is unaffected"
    );
    assert!(
        !effectively_enabled(&scene, child),
        "the disabled panel disables its child"
    );
}

#[test]
fn hit_testing_rejects_a_child_under_a_hidden_ancestor() {
    let (mut scene, panel, child) = nested();
    scene.get_mut(panel).unwrap().visible = false;
    assert!(
        hit_bounds(&scene, child, 108, 60).is_none(),
        "the hidden ancestor's child is not a hit target"
    );
}

#[test]
fn an_ancestors_clip_bounds_a_childs_painted_area() {
    let (view, child) = (WidgetId::from_raw(20), WidgetId::from_raw(21));
    let mut scene = Scene::new();
    let mut view_node = node(window(), Rect::new(0, 0, 100, 80));
    // The view clips its descendants to a 10px band at its top.
    view_node.clip = Some(Rect::new(0, 0, 100, 10));
    scene.insert(view, view_node);
    scene.insert(
        child,
        node(ParentRef::Widget(view), Rect::new(0, 5, 100, 40)),
    );

    let clip = ancestor_clip(&scene, child).expect("the child is clipped");
    let bounds = absolute_bounds(&scene, child).unwrap();
    assert_eq!(
        intersect(bounds, clip),
        Rect::new(0, 5, 100, 10),
        "the child's bounds are chopped to the view's clip"
    );
}

#[test]
fn a_child_outside_its_ancestors_clip_has_nothing_to_paint() {
    let (view, child) = (WidgetId::from_raw(30), WidgetId::from_raw(31));
    let mut scene = Scene::new();
    let mut view_node = node(window(), Rect::new(0, 0, 100, 80));
    view_node.clip = Some(Rect::new(0, 0, 100, 10));
    scene.insert(view, view_node);
    scene.insert(
        child,
        node(ParentRef::Widget(view), Rect::new(0, 20, 100, 40)),
    );

    let clip = ancestor_clip(&scene, child).expect("the child is clipped");
    let bounds = absolute_bounds(&scene, child).unwrap();
    assert!(intersect(bounds, clip).is_empty());
}

#[test]
fn hit_testing_observes_ancestor_clips() {
    let (view, child) = (WidgetId::from_raw(20), WidgetId::from_raw(21));
    let mut scene = Scene::new();
    let mut view_node = node(window(), Rect::new(0, 0, 100, 80));
    view_node.clip = Some(Rect::new(0, 0, 100, 10));
    scene.insert(view, view_node);
    scene.insert(
        child,
        node(ParentRef::Widget(view), Rect::new(0, 0, 100, 40)),
    );

    assert!(
        hit_bounds(&scene, child, 50, 5).is_some(),
        "inside the clip"
    );
    assert!(
        hit_bounds(&scene, child, 50, 20).is_none(),
        "below the clip is not a hit target"
    );
}

#[test]
fn the_topmost_node_under_the_point_wins_with_local_coordinates() {
    let (mut scene, panel, child) = nested();
    let on_top = WidgetId::from_raw(12);
    scene.insert(on_top, node(window(), Rect::new(100, 50, 140, 70)));
    let window_id = WindowId::from_raw(1);
    assert_eq!(hit_test(&scene, window_id, 110, 60), Some((on_top, 10, 10)));
    scene.raise(child);
    scene.raise(panel);
    assert_eq!(
        hit_test(&scene, window_id, 110, 60),
        Some((panel, 10, 10)),
        "a raised node is hit first"
    );
}
