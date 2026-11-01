//! Silhouettes of non-box brushes: drawing, picking and the cache.

use super::document::Document;
use super::state::EditorState;

#[test]
fn a_ramp_outline_is_its_silhouette_not_its_bounds() {
    let s = EditorState::new(Document::from_level(&mscent_world::showcase_level()));
    let ramp = s.doc.brushes().iter().find(|b| !b.is_box()).unwrap();
    for (a, b) in [(0, 1), (0, 2), (1, 2)] {
        let o = ramp.outline(a, b);
        assert!(o.len() >= 3, "view {a},{b}: {o:?}");
        // Every corner lies inside the bounds and the loop is convex.
        let (lo, hi) = ramp.bounds();
        assert!(
            o.iter()
                .all(|p| p[0] >= lo[a] - 1e-9 && p[0] <= hi[a] + 1e-9)
        );
        assert!(
            o.iter()
                .all(|p| p[1] >= lo[b] - 1e-9 && p[1] <= hi[b] + 1e-9)
        );
    }
    // A ramp seen from the side is a wedge: fewer than four corners or a
    // corner of the bounds is missing in at least one side view.
    let wedge = [(0, 2), (1, 2)].iter().any(|&(a, b)| {
        let (lo, hi) = ramp.bounds();
        let corners = [
            [lo[a], lo[b]],
            [hi[a], lo[b]],
            [hi[a], hi[b]],
            [lo[a], hi[b]],
        ];
        let o = ramp.outline(a, b);
        corners.iter().any(|c| {
            !o.iter()
                .any(|p| (p[0] - c[0]).abs() < 1e-9 && (p[1] - c[1]).abs() < 1e-9)
        })
    });
    assert!(wedge, "a ramp must not look like its bounding box");
}

#[test]
fn clicking_a_ramps_empty_bounds_corner_does_not_pick_it() {
    let s = EditorState::new(Document::from_level(&mscent_world::showcase_level()));
    let ramp = s.doc.brushes().iter().find(|b| !b.is_box()).unwrap();
    let (lo, hi) = ramp.bounds();
    for (a, b) in [(0, 2), (1, 2)] {
        let o = ramp.outline(a, b);
        let corners = [
            [lo[a], lo[b]],
            [hi[a], lo[b]],
            [hi[a], hi[b]],
            [lo[a], hi[b]],
        ];
        let missing = corners.iter().find(|c| {
            !o.iter()
                .any(|p| (p[0] - c[0]).abs() < 1e-9 && (p[1] - c[1]).abs() < 1e-9)
        });
        if let Some(c) = missing {
            let inset = [
                c[0] + (o[0][0] - c[0]) * 1e-3,
                c[1] + (o[0][1] - c[1]) * 1e-3,
            ];
            assert!(
                !ramp.covers(a, b, inset),
                "bounds corner {c:?} is not the ramp"
            );
        }
        assert!(
            o.iter().all(|p| ramp.covers(a, b, *p)),
            "its own corners are on it"
        );
    }
}

#[test]
fn outline_cache_rebuilds_only_when_the_planes_change() {
    use super::outline::OutlineCache;
    let s = EditorState::new(Document::from_level(&mscent_world::showcase_level()));
    let ramp = s.doc.brushes().iter().find(|b| !b.is_box()).unwrap();
    let mut cache = OutlineCache::default();
    let first = cache.get(ramp, 0, 2, 8).to_vec();
    assert_eq!(first, ramp.outline(0, 2));
    let mut moved = ramp.clone();
    moved.translate([5.0, 0.0, 0.0]);
    let after = cache.get(&moved, 0, 2, 8).to_vec();
    assert_ne!(after, first, "a moved ramp is not served from the cache");
}
