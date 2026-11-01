use super::*;

fn window() -> WindowId {
    WindowId::from_raw(1)
}

fn node(parent: ParentRef) -> Node {
    Node::new(window(), parent, Rect::new(0, 0, 10, 10))
}

fn ids(scene: &Scene) -> Vec<u64> {
    scene.iter().map(|(id, _)| id.raw()).collect()
}

#[test]
fn nodes_paint_in_creation_order_and_a_raise_moves_one_to_the_top() {
    let mut scene = Scene::new();
    for raw in 1..=4 {
        scene.insert(WidgetId::from_raw(raw), node(ParentRef::Window(window())));
    }
    assert_eq!(ids(&scene), [1, 2, 3, 4]);

    assert!(scene.raise(WidgetId::from_raw(2)));
    assert_eq!(ids(&scene), [1, 3, 4, 2]);
    assert!(
        !scene.raise(WidgetId::from_raw(2)),
        "already on top: nothing moves"
    );
    assert!(!scene.raise(WidgetId::from_raw(99)), "no such node");
}

#[test]
fn removing_a_container_removes_everything_created_inside_it() {
    let mut scene = Scene::new();
    let (panel, row, label, other) = (
        WidgetId::from_raw(1),
        WidgetId::from_raw(2),
        WidgetId::from_raw(3),
        WidgetId::from_raw(4),
    );
    scene.insert(panel, node(ParentRef::Window(window())));
    scene.insert(row, node(ParentRef::Widget(panel)));
    scene.insert(label, node(ParentRef::Widget(row)));
    scene.insert(other, node(ParentRef::Window(window())));

    let mut gone = scene.remove(panel);
    gone.sort_by_key(|id| id.raw());
    assert_eq!(gone, [panel, row, label]);
    assert_eq!(ids(&scene), [4]);
    assert!(!scene.contains(label));
    assert!(
        scene.children(panel).is_empty(),
        "no dangling child list for a removed container"
    );
}

#[test]
fn removing_a_child_unlinks_it_from_its_parent() {
    let mut scene = Scene::new();
    let (panel, a, b) = (
        WidgetId::from_raw(1),
        WidgetId::from_raw(2),
        WidgetId::from_raw(3),
    );
    scene.insert(panel, node(ParentRef::Window(window())));
    scene.insert(a, node(ParentRef::Widget(panel)));
    scene.insert(b, node(ParentRef::Widget(panel)));

    scene.remove(a);
    assert_eq!(scene.children(panel), [b]);
    assert_eq!(scene.subtree(panel).len(), 2);
}

#[test]
fn a_raised_parent_keeps_its_children_and_a_subtree_lists_them() {
    let mut scene = Scene::new();
    let (panel, child) = (WidgetId::from_raw(1), WidgetId::from_raw(2));
    scene.insert(panel, node(ParentRef::Window(window())));
    scene.insert(child, node(ParentRef::Widget(panel)));
    scene.raise(panel);
    assert_eq!(ids(&scene), [2, 1]);
    assert_eq!(scene.subtree(panel), [panel, child]);
}

#[test]
fn closing_a_window_removes_only_its_nodes() {
    let mut scene = Scene::new();
    let second = WindowId::from_raw(2);
    scene.insert(WidgetId::from_raw(1), node(ParentRef::Window(window())));
    scene.insert(
        WidgetId::from_raw(2),
        Node::new(second, ParentRef::Window(second), Rect::new(0, 0, 5, 5)),
    );
    let gone = scene.remove_window(window());
    assert_eq!(gone, [WidgetId::from_raw(1)]);
    assert_eq!(scene.len(), 1);
    assert_eq!(ids(&scene), [2]);
}

/// The old storage: a `Vec` in paint order, searched linearly, with the
/// destroy cascade as a fixed point over "parent is gone".
#[derive(Default)]
struct Model(Vec<(WidgetId, ParentRef)>);

impl Model {
    fn destroy(&mut self, id: WidgetId) {
        self.0.retain(|(node, _)| *node != id);
        loop {
            let gone: Vec<WidgetId> = self
                .0
                .iter()
                .filter(|(_, parent)| match parent {
                    ParentRef::Window(_) => false,
                    ParentRef::Widget(parent) => !self.0.iter().any(|(id, _)| id == parent),
                })
                .map(|(id, _)| *id)
                .collect();
            if gone.is_empty() {
                return;
            }
            self.0.retain(|(id, _)| !gone.contains(id));
        }
    }

    fn raise(&mut self, id: WidgetId) {
        if let Some(at) = self.0.iter().position(|(node, _)| *node == id) {
            let entry = self.0.remove(at);
            self.0.push(entry);
        }
    }
}

/// Random creates, raises and destroys keep the scene's paint order and
/// membership equal to the linear-scan storage it replaces.
#[test]
fn the_scene_matches_the_linear_scan_storage_over_random_operations() {
    for seed in 1..=60u64 {
        let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move |bound: u64| {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            state.wrapping_mul(0x2545_F491_4F6C_DD1D) % bound
        };
        let mut scene = Scene::new();
        let mut model = Model::default();
        let mut created = 0u64;
        for _ in 0..120 {
            match next(10) {
                0..=4 => {
                    created += 1;
                    let id = WidgetId::from_raw(created);
                    let alive: Vec<WidgetId> = model.0.iter().map(|(id, _)| *id).collect();
                    let parent = if alive.is_empty() || next(3) == 0 {
                        ParentRef::Window(window())
                    } else {
                        ParentRef::Widget(alive[next(alive.len() as u64) as usize])
                    };
                    scene.insert(id, node(parent));
                    model.0.push((id, parent));
                }
                5..=6 if !model.0.is_empty() => {
                    let id = model.0[next(model.0.len() as u64) as usize].0;
                    scene.raise(id);
                    model.raise(id);
                }
                _ if !model.0.is_empty() => {
                    let id = model.0[next(model.0.len() as u64) as usize].0;
                    scene.remove(id);
                    model.destroy(id);
                }
                _ => {}
            }
            let want: Vec<u64> = model.0.iter().map(|(id, _)| id.raw()).collect();
            assert_eq!(ids(&scene), want, "seed {seed}");
        }
    }
}
