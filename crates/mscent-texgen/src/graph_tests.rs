use super::*;
use crate::recipe::read;

fn recipe(body: &str) -> Recipe {
    read(format!("miniscent-recipe 1\nsize 16 16\n{body}").as_bytes()).expect("reads")
}

fn size() -> Size {
    Size::new(16, 16).expect("size")
}

fn compile_error(body: &str) -> String {
    match compile(&recipe(body)) {
        Err(TexError::Graph(why)) => why,
        other => panic!("expected a graph error, got {other:?}"),
    }
}

fn cached(cache: &mut Cache) -> RunOptions<'_> {
    RunOptions {
        cache: Some(cache),
        ..Default::default()
    }
}

const DIAMOND: &str = "
op 0 0 2 noise seed=5
op 0 1 1 store name=a
op 1 1 1 blur radius=0.1
op 0 2 2 merge mode=max
op 0 3 1 out-albedo
op 1 3 1 load name=a
op 1 4 1 out-height
";

#[test]
fn a_block_reads_what_overlaps_its_columns_left_to_right() {
    let r = recipe(
        "
op 3 0 1 flat
op 0 0 1 noise
op 0 1 4 merge
",
    );
    let g = compile(&r).expect("compiles");
    assert_eq!(g.inputs_of(2), &[1, 0]);
}

#[test]
fn a_generator_under_a_block_starts_a_new_chain() {
    let r = recipe("op 0 0 1 flat\nop 0 1 1 noise\n");
    let g = compile(&r).expect("compiles");
    assert!(g.inputs_of(1).is_empty());
}

#[test]
fn wide_blocks_feed_every_block_under_them() {
    let g = compile(&recipe(DIAMOND)).expect("compiles");
    assert_eq!(g.inputs_of(1), &[0]);
    assert_eq!(g.inputs_of(2), &[0]);
    assert_eq!(g.inputs_of(3), &[1, 2]);
    assert_eq!(g.inputs_of(5), &[1], "load reads the store");
    assert_eq!(g.output(Channel::Albedo), Some(4));
    assert_eq!(g.output(Channel::Height), Some(6));
    assert_eq!(g.output(Channel::Specular), None);
    assert_eq!(g.output(Channel::Alpha), None);
}

#[test]
fn pages_that_do_not_connect_are_errors() {
    for (body, why) in [
        ("op 0 0 1 blur\n", "more inputs"),
        ("op 0 0 1 flat\nop 0 1 1 merge\n", "more inputs"),
        ("op 0 0 1 flat\nop 1 0 1 flat\nop 0 1 2 blur\n", "too many"),
        ("op 0 0 1 flat\nop 0 0 1 noise\n", "overlaps"),
        ("op 0 0 2 flat\nop 1 0 1 noise\n", "overlaps"),
        ("op 64 0 1 flat\n", "off the page"),
        ("op 0 256 1 flat\n", "off the page"),
        ("op 0 0 0 flat\n", "off the page"),
        ("op 65535 0 1 flat\n", "off the page"),
        ("op 0 0 65535 flat\n", "off the page"),
        ("op 0 0 1 load name=b\n", "nothing stores"),
        (
            "op 0 0 1 flat\nop 0 1 1 store name=a\nop 1 0 1 flat\nop 1 1 1 store name=a\n",
            "already used",
        ),
        (
            "op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-albedo\n",
            "repeats",
        ),
    ] {
        let got = compile_error(body);
        assert!(got.contains(why), "{body}: {got}");
    }
}

#[test]
fn a_cycle_through_store_and_load_is_an_error() {
    let got = compile_error(
        "
op 0 0 1 load name=b
op 0 1 1 store name=a
op 1 0 1 load name=a
op 1 1 1 store name=b
",
    );
    assert!(got.contains("cycle"), "{got}");
}

#[test]
fn a_run_gives_the_outputs_and_frees_the_rest() {
    let r = recipe(DIAMOND);
    let g = compile(&r).expect("compiles");
    let out = run(&r, &g, size(), RunOptions::default()).expect("runs");
    let albedo = out.image(4).expect("albedo");
    assert_eq!(albedo.size(), size());
    let height = out.image(6).expect("height");
    assert!(
        albedo.pixels() != height.pixels(),
        "max with a blur differs"
    );
    assert!(out.image(0).is_none(), "the source was freed");
    assert!(out.image(3).is_none(), "the merge was freed");
}

#[test]
fn a_kept_block_stays() {
    let r = recipe(DIAMOND);
    let g = compile(&r).expect("compiles");
    let options = RunOptions {
        keep: &[2],
        ..Default::default()
    };
    let out = run(&r, &g, size(), options).expect("runs");
    assert!(out.image(2).is_some());
}

#[test]
fn a_run_with_a_cache_gives_the_same_bytes_and_skips_the_work() {
    let r = recipe(DIAMOND);
    let g = compile(&r).expect("compiles");
    let plain = run(&r, &g, size(), RunOptions::default()).expect("runs");
    let mut cache = Cache::new(1 << 24);
    let first = run(&r, &g, size(), cached(&mut cache)).expect("runs");
    assert_eq!(plain.image(4), first.image(4));
    let before = cache.len();
    let again = run(&r, &g, size(), cached(&mut cache)).expect("runs");
    assert_eq!(cache.len(), before, "no new results");
    assert!(
        Arc::ptr_eq(
            first.image(4).expect("first"),
            again.image(4).expect("again")
        ),
        "served from the cache"
    );
}

#[test]
fn changing_a_parameter_reruns_only_what_depends_on_it() {
    let mut r = recipe(
        "
op 0 0 1 noise seed=1
op 0 1 1 blur
op 1 0 1 noise seed=2
op 1 1 1 blur radius=0.1
op 0 2 2 merge
op 0 3 1 out-albedo
",
    );
    let g = compile(&r).expect("compiles");
    let mut cache = Cache::new(1 << 24);
    let mut go = |r: &Recipe| {
        let options = RunOptions {
            cache: Some(&mut cache),
            keep: &[1, 3],
            ..Default::default()
        };
        run(r, &g, size(), options).expect("runs")
    };
    let first = go(&r);
    let spec = r.blocks[0].spec;
    r.blocks[0]
        .params
        .set(spec, "seed", Value::Int(9))
        .expect("seed");
    let second = go(&r);
    let (a, b) = (first.image(3).expect("a"), second.image(3).expect("b"));
    assert!(Arc::ptr_eq(a, b), "the other branch was not run again");
    assert_ne!(first.image(1), second.image(1), "this branch was");
}

#[test]
fn a_full_cache_drops_the_oldest() {
    let one = Size::new(16, 16).expect("size").bytes();
    let mut cache = Cache::new(one * 2);
    let image = || Arc::new(Image::flat(size(), [0; 4]));
    cache.put(1, image());
    cache.put(2, image());
    assert!(cache.get(1).is_some(), "touches 1");
    cache.put(3, image());
    assert_eq!(cache.len(), 2);
    assert!(cache.get(2).is_none(), "2 was the oldest");
    assert!(cache.get(1).is_some());
}

#[test]
fn a_set_cancel_flag_stops_the_run() {
    let r = recipe(DIAMOND);
    let g = compile(&r).expect("compiles");
    let cancel = AtomicBool::new(true);
    let options = RunOptions {
        cancel: Some(&cancel),
        ..Default::default()
    };
    assert_eq!(
        run(&r, &g, size(), options).err(),
        Some(TexError::Cancelled)
    );
}

/// 92 identical sources at 1024 px, 8 MiB each, which 13 merges cannot read until a
/// source last in page order has run: 736 MiB. Ready blocks run in page order, so
/// all the sources run first. A cache serves them with one image, and that used to
/// pass where a bake, with none, failed, and stopped every material of a directory.
fn held_open_page() -> Recipe {
    let mut body = String::new();
    let mut flats = 0;
    for merge in 0..13 {
        let y = merge * 2;
        for x in 0..7 {
            if flats < 92 {
                body.push_str(&format!("op {x} {y} 1 flat\n"));
                flats += 1;
            }
        }
        body.push_str(&format!("op 7 {y} 1 load name=late\n"));
        body.push_str(&format!("op 0 {} 8 merge\n", y + 1));
    }
    body.push_str("op 20 0 1 noise\nop 20 1 1 store name=late\n");
    body.push_str("op 40 0 1 flat\nop 40 1 1 out-albedo\n");
    read(format!("miniscent-recipe 1\nsize 1024 1024\n{body}").as_bytes()).expect("reads")
}

#[test]
fn the_memory_plan_does_not_depend_on_a_cache() {
    let r = held_open_page();
    let g = compile(&r).expect("compiles");
    assert!(peak_images(&r, &g, &[]) * r.size.bytes() > MAX_LIVE_BYTES);
    let plain = run(&r, &g, r.size, RunOptions::default());
    assert_eq!(plain.err(), Some(TexError::TooLarge));
    let mut cache = Cache::new(1 << 30);
    let with_cache = run(&r, &g, r.size, cached(&mut cache));
    assert_eq!(with_cache.err(), Some(TexError::TooLarge));
    assert!(cache.is_empty(), "refused before any kernel ran");
    // A draft of the same page is small enough, and the same with or without.
    let draft = Size::new(64, 64).expect("size");
    assert!(run(&r, &g, draft, RunOptions::default()).is_ok());
    assert!(run(&r, &g, draft, cached(&mut cache)).is_ok());
}

#[test]
fn the_memory_plan_matches_what_a_run_holds() {
    // A store holds the flat's image and makes none, and it keeps that image alive
    // until the merge has read it, after the blur that also reads it. The merge is
    // read by nothing and freed, and only then does the last flat run.
    let page = "
op 0 0 2 flat
op 0 1 1 store name=a
op 1 1 1 blur
op 0 2 2 merge
op 2 0 1 flat
op 2 1 1 out-albedo
";
    let r = recipe(page);
    let g = compile(&r).expect("compiles");
    assert_eq!(peak_images(&r, &g, &[]), 3, "flat, blur, merge");
    assert_eq!(
        peak_images(&r, &g, &[0, 2, 3, 4]),
        4,
        "kept, and the last flat"
    );
    // Nothing is read here and nothing is kept: one image at a time.
    let unread = recipe("op 0 0 1 flat\nop 1 0 1 flat\n");
    let g = compile(&unread).expect("compiles");
    assert_eq!(peak_images(&unread, &g, &[]), 1);
}

/// Time is checked like memory, before anything runs.
#[test]
fn a_page_past_the_work_limit_is_refused_at_its_size_only() {
    let mut body = String::new();
    for x in 0..64 {
        for y in 0..4 {
            body.push_str(&format!("op {x} {y} 1 noise octaves=8\n"));
        }
    }
    let r = read(format!("miniscent-recipe 1\nsize 1024 1024\n{body}").as_bytes()).expect("reads");
    let g = compile(&r).expect("compiles");
    assert!(page_cost(&r, &g, r.size) > MAX_RECIPE_NS);
    let mut cache = Cache::new(1 << 24);
    assert_eq!(
        run(&r, &g, r.size, cached(&mut cache)).err(),
        Some(TexError::TooLarge)
    );
    assert!(cache.is_empty(), "refused before any kernel ran");
    let draft = Size::new(16, 16).expect("size");
    assert!(page_cost(&r, &g, draft) < MAX_RECIPE_NS);
    assert!(run(&r, &g, draft, RunOptions::default()).is_ok());
}

#[test]
fn a_page_kept_whole_that_would_hold_too_much_is_an_error_before_allocating() {
    // 256 sources at 1024 px, nothing reads them, and the caller asks to keep them
    // all: 8 MiB each, past the limit.
    let mut body = String::new();
    for x in 0..64 {
        for y in 0..4 {
            body.push_str(&format!("op {x} {y} 1 flat\n"));
        }
    }
    let r = read(format!("miniscent-recipe 1\nsize 1024 1024\n{body}").as_bytes()).expect("reads");
    let g = compile(&r).expect("compiles");
    let keep: Vec<usize> = (0..256).collect();
    let options = RunOptions {
        keep: &keep,
        ..Default::default()
    };
    let got = run(&r, &g, r.size, options);
    assert_eq!(got.err(), Some(TexError::TooLarge));
}

#[test]
fn a_recipe_runs_at_any_size() {
    let r = recipe(DIAMOND);
    let g = compile(&r).expect("compiles");
    let big = Size::new(64, 64).expect("size");
    let out = run(&r, &g, big, RunOptions::default()).expect("runs");
    assert_eq!(out.image(4).expect("albedo").size(), big);
}

/// Editing marks what baking refuses, and nothing else.
#[test]
fn problems_are_found_exactly_when_a_page_does_not_compile() {
    let pages = [
        "op 0 0 1 flat\nop 0 1 1 out-albedo\n",
        "op 0 0 1 flat\nop 0 1 2 merge\n",
        "op 0 0 1 flat\nop 1 0 1 flat\nop 0 1 2 merge\nop 0 2 1 out-albedo\n",
        "op 0 0 1 flat\nop 0 0 1 flat\n",
        "op 65535 0 65535 flat\n",
        "op 0 0 0 flat\n",
        "op 0 300 1 flat\n",
        "op 0 0 1 load\n",
        "op 0 0 1 flat\nop 0 1 1 store\nop 0 2 1 load\nop 0 3 1 out-albedo\n",
        "op 0 0 1 load\nop 0 1 1 store\n",
        "op 0 0 1 flat\nop 0 1 1 store\nop 1 0 1 flat\nop 1 1 1 store\n",
        "op 0 0 1 flat\nop 0 1 1 out-albedo\nop 1 0 1 flat\nop 1 1 1 out-albedo\n",
    ];
    for page in pages {
        let r = recipe(page);
        let found = problems(&r);
        assert_eq!(found.is_empty(), compile(&r).is_ok(), "{page}: {found:?}");
    }
}

#[test]
fn problems_name_the_blocks_they_belong_to() {
    let blocks = |page| -> Vec<usize> { problems(&recipe(page)).iter().map(|p| p.block).collect() };
    assert_eq!(blocks("op 0 0 1 flat\nop 5 5 1 out-albedo\n"), [1]);
    assert_eq!(blocks("op 0 0 1 flat\nop 0 0 1 flat\n"), [0, 1]);
    assert_eq!(blocks("op 0 0 1 load\nop 0 1 1 store\n"), [0, 1]);
    assert_eq!(blocks("op 0 0 1 flat\nop 70 0 1 flat\n"), [1]);
}

#[test]
fn a_failed_kernel_is_named() {
    // These compile, and fail only when they run: a count that cannot tile is a
    // kernel's to find out.
    for (body, why) in [
        (
            "op 0 0 1 flat\nop 4 4 1 checker cols=3\n",
            TexError::NotTiling("checker counts must be even"),
        ),
        (
            "op 0 0 1 flat\nop 4 4 1 bricks rows=3 offset=0.5\n",
            TexError::NotTiling("odd brick rows cannot alternate"),
        ),
    ] {
        let page = recipe(body);
        let graph = compile(&page).expect("compiles");
        let failed = Cell::new(None);
        let options = RunOptions {
            failed: Some(&failed),
            ..Default::default()
        };
        assert_eq!(run(&page, &graph, size(), options).err(), Some(why));
        assert_eq!(failed.get(), Some(1));
    }
}
