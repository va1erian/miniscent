//! Recipes as they may come off a disk, opened in the workspace: refused with a
//! message, or open and marked broken. The editor keeps working either way.

use mscent_texgen::recipe::{self, MAX_FILE};

use super::tests::{open_and_wait, settle, store};
use super::*;

/// A directory with `brick.mst` holding `bytes`.
fn with_brick(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("mscent-hostile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("dir");
    std::fs::write(d.join("brick.mst"), bytes).expect("recipe");
    d
}

fn opened(name: &str, bytes: &[u8]) -> (TexStudio, std::path::PathBuf) {
    let mut s = TexStudio::new(store());
    let d = with_brick(name, bytes);
    s.store.set_dir(d.clone());
    s.set_open(true);
    // What the opening said, if anything, waits for the test to ask for it.
    let said = open_and_wait(&mut s, "brick");
    s.message = said;
    (s, d)
}

#[test]
fn a_file_that_is_no_recipe_is_a_message_and_nothing_opens() {
    let big = vec![b'x'; MAX_FILE + 10];
    let cases: [&[u8]; 5] = [
        b"",
        b"not a recipe",
        b"miniscent-recipe 1\nsize 3 3\n",
        b"miniscent-recipe 1\nsize 16 16\nop 0 0 1 no-such-op\n",
        &big,
    ];
    for (i, bytes) in cases.into_iter().enumerate() {
        let (mut s, d) = opened(&format!("bad{i}"), bytes);
        assert!(s.doc().is_none(), "case {i}");
        let said = s.tick().message.expect("says why");
        assert!(said.contains("brick.mst"), "{said}");
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn a_page_that_cannot_compile_opens_marked_and_asks_for_nothing() {
    let pages = [
        // Off the page by every means, and past what a u16 can add.
        "op 65535 65535 65535 flat\nop 65534 3 65535 out-albedo\n",
        "op 0 0 0 flat\nop 0 1 1 out-albedo\n",
        // A cycle, a repeated store, a repeated output, a load of nothing.
        "op 0 0 1 load name=a\nop 0 1 1 store name=a\n",
        "op 0 0 1 flat\nop 0 1 1 store name=a\nop 1 0 1 flat\nop 1 1 1 store name=a\n",
        "op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-albedo\n",
        "op 0 0 1 load name=z\n",
        // Every block on top of the first.
        "op 0 0 1 flat\nop 0 0 1 flat\nop 0 0 1 flat\n",
    ];
    for (i, page) in pages.into_iter().enumerate() {
        let text = format!("miniscent-recipe 1\nsize 64 64\n{page}");
        let (mut s, d) = opened(&format!("page{i}"), text.as_bytes());
        let (blocks, why) = s.trouble();
        assert!(!blocks.is_empty(), "{page}");
        assert!(why.is_some(), "{page}");
        let (picture, _) = settle(&mut s);
        assert!(picture.is_none(), "{page}: nothing evaluated");
        assert!(!s.apply(), "{page}: nothing to apply");
        let _ = std::fs::remove_dir_all(d);
    }
}

#[test]
fn a_page_at_the_cap_of_blocks_evaluates() {
    // The most blocks a page holds, as 64 chains of a flat and three blurs: the
    // page must evaluate, and every block must get a thumbnail. Not a time test.
    let mut page = String::new();
    for x in 0..64 {
        page.push_str(&format!("op {x} 0 1 flat\n"));
        for y in 1..4 {
            page.push_str(&format!("op {x} {y} 1 blur radius=0.01\n"));
        }
    }
    let text = format!("miniscent-recipe 1\nsize 128 128\n{page}");
    let (mut s, d) = opened("cap", text.as_bytes());
    assert_eq!(s.doc().expect("open").recipe().blocks.len(), 256);
    assert!(s.doc().expect("open").page().is_ok());
    s.set_tab(Tab::Block);
    let (_, thumbs) = settle(&mut s);
    assert_eq!(thumbs.expect("thumbnails").images.len(), 256);
    let _ = std::fs::remove_dir_all(d);
}

/// A request whose page and graph do not belong together, which is the only way
/// to make an operator panic: no real page can. `graph_of` is compiled, and run
/// against `page`.
fn forged(s: &mut TexStudio, graph_of: &str, page: &str) {
    let read = |body: &str| {
        recipe::read(format!("miniscent-recipe 1\nsize 16 16\n{body}").as_bytes()).expect("reads")
    };
    s.seq += 1;
    s.asked_seq = s.seq;
    let request = Request {
        seq: s.seq,
        recipe: read(page),
        graph: mscent_texgen::graph::compile(&read(graph_of)).expect("compiles"),
        selected: None,
        tab: s.tab,
        draft_only: false,
        emissive: false,
    };
    assert!(s.worker.as_ref().expect("worker").post(request).is_ok());
}

fn wait_for_failure(s: &mut TexStudio) -> Failure {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let tick = s.tick();
        assert!(tick.picture.is_none(), "the last good picture stays");
        if let Some(failure) = &s.failure {
            return failure.clone();
        }
        assert!(std::time::Instant::now() < end, "no failure came");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn a_kernel_that_fails_marks_its_block_and_the_last_good_picture_stays() {
    let text = "miniscent-recipe 1\nsize 16 16\nop 0 0 1 checker cols=4\nop 0 1 1 out-albedo\n";
    let (mut s, d) = opened("kernel", text.as_bytes());
    s.set_tab(Tab::Albedo);
    settle(&mut s);
    // An odd count is in range and the page compiles, but the kernel cannot tile.
    let odd = mscent_texgen::ops::Value::Int(3);
    s.edit(|doc| doc.set_param(0, 0, odd, false))
        .expect("edits");
    let failure = wait_for_failure(&mut s);
    assert_eq!(failure.block, Some(0));
    assert_eq!(s.trouble().0, [0]);
    assert!(s.trouble().1.is_some_and(|why| why.contains("even")));
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn a_panic_on_the_worker_is_a_message_and_the_worker_goes_on() {
    let text = "miniscent-recipe 1\nsize 16 16\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";
    let (mut s, d) = opened("panic", text.as_bytes());
    s.set_tab(Tab::Albedo);
    let first = settle(&mut s).0.expect("a picture").rgba;
    // Block 0 of this graph reads block 1, which this page does not have.
    forged(
        &mut s,
        "op 0 1 1 out-albedo\nop 0 0 1 flat\n",
        "op 0 0 1 flat\n",
    );
    let failure = wait_for_failure(&mut s);
    assert!(failure.why.contains("failed"), "{}", failure.why);
    // Any edit evaluates again, on the same thread.
    s.edit(|doc| {
        doc.set_gloss(1, false);
        Ok(())
    });
    assert_eq!(settle(&mut s).0.expect("a picture").rgba, first);
    assert!(s.failure.is_none(), "a later picture clears it");
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn the_manifests_gloss_wins_over_the_recipes() {
    let text = "miniscent-recipe 1\nsize 16 16\ngloss 200\nop 0 0 1 flat\nop 0 1 1 out-albedo\n";
    let (s, d) = opened("gloss", text.as_bytes());
    assert_eq!(s.doc().expect("open").recipe().gloss, 40);
    let _ = std::fs::remove_dir_all(d);
}

#[test]
fn extreme_parameter_values_from_a_file_are_refused_at_the_door() {
    let cases = [
        "op 0 0 1 noise octaves=999999\n",
        "op 0 0 1 noise freq_x=-5\n",
        "op 0 0 1 noise persistence=NaN\n",
        "op 0 0 1 flat color=9,9,9\n",
    ];
    for (i, page) in cases.into_iter().enumerate() {
        let text = format!("miniscent-recipe 1\nsize 16 16\n{page}");
        let (mut s, d) = opened(&format!("param{i}"), text.as_bytes());
        assert!(s.doc().is_none(), "{page}");
        assert!(s.tick().message.is_some(), "{page}");
        let _ = std::fs::remove_dir_all(d);
    }
}
