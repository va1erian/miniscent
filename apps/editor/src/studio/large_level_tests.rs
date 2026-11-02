//! The largest level the generator makes, through the whole editor path.

use std::time::Duration;

use super::preview_tests::{bakes, tick_until, tick_within};
use super::*;
use crate::build::{self, Light};

/// 20 s on 24 cores at the real settings, so an unoptimised run is out of the
/// question; `layout.rs` covers this level's atlas there.
#[test]
#[cfg_attr(debug_assertions, ignore = "needs --release: 5.8 million luxels")]
fn a_level_too_tall_for_the_old_atlas_reaches_full_light_at_its_own_luxel_size() {
    let params = mscent_world::GenParams {
        cols: 8,
        rows: 8,
        ..mscent_world::GenParams::default()
    };
    let doc = Document::from_level(&mscent_world::generate_level(7, &params));
    let mut studio = Studio::new(&doc);
    let shown = tick_until(&mut studio, &doc, |s| s.is_current(&doc));
    let limit = Duration::from_secs(900);
    let w = tick_within(&mut studio, &doc, limit, |s| {
        s.preview(&doc) == Preview::Full
    });
    println!(
        "{} brushes: first picture {:?}, full light {:?} later, worst UI tick {:?}",
        doc.brushes().len(),
        shown.took,
        w.took,
        shown.costs.worst().max(w.costs.worst())
    );
    shown.costs.merged(&w.costs).assert_cheap();
    assert!(w.states.contains("Baking"), "{:?}", w.states);
    assert!(!w.log.iter().any(|l| l.starts_with("ERROR")), "{:?}", w.log);
    assert!(
        !w.log.iter().any(|l| l.contains("coarsened")),
        "{:?}",
        w.log
    );
    assert_eq!(bakes(&studio), 1);
    let cache = studio.cache();
    let job =
        build::start_preview(&doc, &cache, &Default::default(), &studio.settings).expect("starts");
    let built = loop {
        if let Some(r) = job.poll() {
            break r.expect("builds");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(built.light, Light::Full);
    let map = built.renderer().lightmap();
    assert!(map.atlas().height() > 4096, "{}", map.atlas().height());
    assert_eq!(map.finest_luxel(), Some(studio.settings.build.luxel_size));
}
