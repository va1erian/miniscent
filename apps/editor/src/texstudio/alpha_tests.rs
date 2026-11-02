//! A material with an `out-alpha`, through the studio: it opens, previews over a
//! backdrop and saves with its mask.

use std::sync::atomic::AtomicBool;

use mscent_texgen::recipe;

use super::tests::{set_page, settle, studio};
use super::*;

#[test]
fn a_masked_recipe_previews_over_a_backdrop_and_saves_with_its_mask() {
    let mut s = studio();
    let page = "op 0 0 1 flat color=1,0,0
op 0 1 1 out-albedo
op 1 0 1 rect x=0.75 y=0.5 w=0.5 h=1
op 1 1 1 out-alpha
";
    set_page(&mut s, 64, page);
    let recipe = s.doc().expect("open").recipe().clone();
    for tab in [Tab::Albedo, Tab::Lit] {
        s.set_tab(tab);
        let picture = settle(&mut s).0.expect("a picture");
        let texel = |x: usize, y: usize| &picture.rgba[(y * 64 + x) * 4..][..3];
        // Solid on the right, where the shape is: red. Clear on the left: grey.
        assert!(texel(60, 4)[0] > texel(60, 4)[1] + 40, "{tab:?} solid");
        let grey = texel(2, 4);
        assert!(grey[0] == grey[1] && grey[1] == grey[2], "{tab:?} {grey:?}");
    }
    // Opening, previewing and saving all keep the mask: the saved text is read
    // back by the game's loader path.
    let texture = eval::bake_saved(&recipe, &AtomicBool::new(false)).expect("saves");
    assert!(texture.is_masked());
    assert_eq!((texture.alpha_at(60, 4), texture.alpha_at(2, 4)), (255, 0));
    assert!(recipe::write(&recipe).contains("out-alpha"));
}
