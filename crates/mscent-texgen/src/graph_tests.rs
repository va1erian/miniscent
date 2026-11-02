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

#[test]
fn a_page_that_would_hold_too_much_is_an_error_before_allocating() {
    // 256 sources at 1024 px, all kept until the end by one wide merge chain:
    // 8 MB each, so past the limit partway.
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
    // No graph compile gives can make a kernel fail on its inputs, so this is a
    // graph from another page: it says the blur at block 1 has no input.
    let graph = compile(&recipe("op 0 0 1 flat\nop 4 4 1 flat\n")).expect("compiles");
    let page = recipe("op 0 0 1 flat\nop 4 4 1 blur\n");
    let failed = Cell::new(None);
    let options = RunOptions {
        failed: Some(&failed),
        ..Default::default()
    };
    assert!(run(&page, &graph, size(), options).is_err());
    assert_eq!(failed.get(), Some(1));
}
