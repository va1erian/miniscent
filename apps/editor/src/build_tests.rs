use super::*;
use mscent_world::Op;

fn wait<T>(job: &Job<T>) -> Result<T, String> {
    loop {
        if let Some(r) = job.poll() {
            return r;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn invalid_brushes_and_empty_levels_are_errors_that_name_the_problem() {
    assert!(
        validate(&Document::new())
            .unwrap_err()
            .contains("no brushes")
    );
    let mut state =
        crate::model::EditorState::new(Document::from_level(&mscent_world::showcase_level()));
    // A zero-size box is refused by the model, so push one in through
    // the document's own replace path to model a corrupt document.
    state.add_box([0.0; 3], [1.0, 1.0, 1.0]).expect("added");
    let id = state.doc.brushes().last().expect("brush").id;
    let mut def = state.doc.brush(id).expect("alive").clone();
    def.shape = crate::model::document::Shape::Box {
        min: [1.0, 1.0, 1.0],
        max: [1.0, 1.0, 1.0],
    };
    let mut doc = state.doc.clone();
    doc.replace_brush_for_test(&def);
    let err = validate(&doc).unwrap_err();
    assert!(err.contains("is invalid"), "{err}");
}

#[test]
fn a_level_without_start_or_goal_previews_view_only() {
    let room = mscent_world::Brush::cuboid(
        mscent_math::Vec3d::new(-4.0, -4.0, -1.0),
        mscent_math::Vec3d::new(4.0, 4.0, 3.0),
        Op::Subtract,
        0,
    )
    .expect("room");
    let doc = Document::from_level(&Level {
        brushes: vec![room],
        entities: vec![],
        ..Default::default()
    });
    let job =
        start_preview(&doc, &SharedCache::default(), &SessionConfig::default()).expect("starts");
    let built = wait(&job).expect("builds");
    assert!(built.unplayable().is_some());
}

#[test]
fn a_script_beside_the_level_decides_whether_it_plays() {
    let text = "miniscent-level 1
box subtract 0 1  -8 -8 -8  8 8 8
player 0 0 0 0
";
    let level = mscent_world::level_text::from_text(text).expect("level");
    let dir = std::env::temp_dir().join(format!("mscent-build-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let mut doc = Document::from_level(&level);
    let cache = SharedCache::default();
    let cfg = SessionConfig::default();
    let build = |doc: &Document| wait(&start_preview(doc, &cache, &cfg).expect("starts"));

    let without = build(&doc).expect("builds");
    assert!(without.unplayable().is_some(), "no script, no goal");
    doc.set_level_path(Some(dir.join("l.msl")));
    std::fs::write(
        dir.join("l.st"),
        "say: 'hello'.
",
    )
    .expect("writes");
    let scripted = build(&doc).expect("builds");
    assert_eq!(scripted.unplayable(), None, "a script can decide the win");
    std::fs::write(
        dir.join("l.st"),
        "marker: 'gone'.
",
    )
    .expect("writes");
    let broken = build(&doc).expect("builds");
    let why = broken.unplayable().expect("a broken script does not play");
    assert!(why.contains("l.st:1:"), "{why}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_showcase_previews_playable_on_a_draft_and_a_bake_makes_it_full() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let cache = SharedCache::default();
    let first = wait(&start_preview(&doc, &cache, &SessionConfig::default()).expect("starts"))
        .expect("builds");
    assert!(first.unplayable().is_none());
    assert_eq!(first.light, Light::Draft);
    assert_eq!(
        cache.lock().expect("cache").bakes(),
        0,
        "a draft is no bake"
    );

    let job = start_bake(&doc, first.light_key, &cache, &SessionConfig::default()).expect("starts");
    let report = wait(&job.job).expect("bakes").expect("not cancelled");
    assert_eq!(report.source, Source::Baked);

    let second = wait(&start_preview(&doc, &cache, &SessionConfig::default()).expect("starts"))
        .expect("builds");
    assert_eq!(second.light, Light::Full);
    assert_eq!(second.light_key, first.light_key);
    assert_eq!(cache.lock().expect("cache").bakes(), 1);
}

#[test]
fn dropping_a_bake_job_cancels_it() {
    let doc = Document::from_level(&mscent_world::showcase_level());
    let cache = SharedCache::default();
    let cancel = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&cancel);
    let copy = Arc::clone(&cache);
    let cfg = SessionConfig::default();
    let job = spawn("test-bake", &doc, move |d| bake(d, &copy, &cfg, &flag)).expect("starts");
    assert!(wait(&job).expect("runs").is_none(), "reported as cancelled");
    assert_eq!(cache.lock().expect("cache").bakes(), 0, "nothing stored");

    let job = start_bake(&doc, 0, &cache, &SessionConfig::default()).expect("starts");
    let flag = Arc::clone(&job.cancel);
    drop(job);
    assert!(flag.load(Ordering::Relaxed));
}
