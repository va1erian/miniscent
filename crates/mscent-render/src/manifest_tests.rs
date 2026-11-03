use super::*;

/// Five materials, so that retired ids can follow.
const STOCK_LINES: &str = "0 a 1\n1 b 2\n2 c 3\n3 d 4\n4 e 5\n";

fn line_error(text: &str) -> LineError {
    match parse(text.as_bytes()) {
        Err(ManifestError::Line(_, why)) => why,
        other => panic!("{text:?} gave {other:?}"),
    }
}

fn entry(name: &str, gloss: u8) -> Entry {
    Entry::Material {
        name: name.to_string(),
        gloss,
        depth: None,
    }
}

#[test]
fn a_new_material_goes_at_the_end_and_a_known_one_keeps_its_id_and_the_comments_stay() {
    let text = format!("# keep me\n{STOCK_LINES}5 retired");
    let (added, id) = with_material(&text, "new", 7).expect("adds");
    assert_eq!(id, 6);
    assert!(added.starts_with("# keep me\n0 a 1\n"), "{added}");
    assert!(added.ends_with("5 retired\n6 new 7\n"), "{added}");
    let (changed, id) = with_material(&added, "c", 200).expect("changes");
    assert_eq!(id, 2);
    let want = added.replace("2 c 3\n", "2 c 200\n");
    assert_eq!(changed, want);
    assert_eq!(parse(changed.as_bytes()).expect("parses").len(), 7);
}

#[test]
fn a_manifest_that_would_not_load_is_never_returned() {
    for bad in ["CON", "a/b", "a.b", "a_n", "retired", "", "../x", "con"] {
        assert!(with_material(STOCK_LINES, bad, 1).is_err(), "{bad}");
    }
    assert!(
        with_material("0 a x\n", "b", 1).is_err(),
        "a bad row stays bad"
    );
    let full: String = (0..MAX_MATERIALS)
        .map(|i| format!("{i} m{i} 1\n"))
        .collect();
    assert!(with_material(&full, "one-more", 1).is_err());
    assert!(
        with_material(&full, "m3", 9).is_ok(),
        "room to change a row"
    );
}

#[test]
fn a_listing_becomes_text_that_parses_to_it() {
    // Stock ids cannot be retired, so the retired one comes after them.
    let mut entries: Vec<Entry> = ["a", "b", "c", "d", "e"].map(|n| entry(n, 1)).into();
    entries.extend([Entry::Retired, entry("f", 3)]);
    assert_eq!(
        parse(text_of(&entries).as_bytes()).expect("parses"),
        entries
    );
}

/// A fresh directory holding empty files of these names.
fn directory_of(tag: &str, files: &[String]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("mscent-listing-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    for file in files {
        std::fs::write(dir.join(file), b"").expect("write");
    }
    dir
}

fn listed_names(dir: &Path) -> Vec<String> {
    from_listing(dir)
        .expect("lists")
        .into_iter()
        .filter_map(|e| match e {
            Entry::Material { name, .. } => Some(name),
            Entry::Retired => None,
        })
        .collect()
}

/// Each of these names a manifest would refuse, so in a listing each would take an
/// id, and move the ones after it, for a material no manifest can list.
#[test]
fn a_listing_skips_recipes_whose_names_a_manifest_refuses() {
    let bad = [
        ".mst",
        "..mst",
        "Foo Bar.mst",
        "_hidden.mst",
        "a_n.mst",
        "retired.mst",
        "con.mst",
        "-a.mst",
        "UPPER.mst",
    ];
    let mut files: Vec<String> = bad.iter().map(ToString::to_string).collect();
    files.extend(["zinc.mst", "ash.mst", "brick.mst"].map(String::from));
    let dir = directory_of("names", &files);
    assert_eq!(listed_names(&dir), ["brick", "ash", "zinc"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_listing_keeps_the_png_rules_it_had() {
    let files = ["a.png", "a_n.png", "_sheet.png", "b.mst", "b.png"].map(String::from);
    let dir = directory_of("pngs", &files);
    assert_eq!(listed_names(&dir), ["a", "b"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_listing_holds_to_the_most_a_manifest_may() {
    let files: Vec<String> = (0..MAX_MATERIALS).map(|i| format!("m{i}.mst")).collect();
    let dir = directory_of("full", &files);
    assert_eq!(listed_names(&dir).len(), MAX_MATERIALS);
    std::fs::write(dir.join("one-more.mst"), b"").expect("write");
    assert!(matches!(
        from_listing(&dir),
        Err(TextureError::Manifest(ManifestError::TooMany))
    ));
    // A refused name is not an entry, so it does not count.
    std::fs::remove_file(dir.join("one-more.mst")).expect("remove");
    std::fs::write(dir.join("_hidden.mst"), b"").expect("write");
    assert_eq!(listed_names(&dir).len(), MAX_MATERIALS);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ids_come_from_the_lines_and_retired_ones_keep_their_place() {
    let text = format!("# a comment\n\n{STOCK_LINES}5 retired\r\n  6\ta-b-2  255\n");
    let mut want = vec![
        entry("a", 1),
        entry("b", 2),
        entry("c", 3),
        entry("d", 4),
        entry("e", 5),
    ];
    want.extend([Entry::Retired, entry("a-b-2", 255)]);
    assert_eq!(parse(text.as_bytes()).unwrap(), want);
}

#[test]
fn ids_must_count_up_with_no_gap_or_repeat() {
    assert_eq!(line_error("1 brick 25\n"), LineError::Gap);
    assert_eq!(line_error("0 brick 25\n2 floor 90\n"), LineError::Gap);
    assert_eq!(
        line_error("0 brick 25\n0 floor 90\n"),
        LineError::DuplicateId
    );
    assert_eq!(
        line_error("0 brick 25\n1 floor 90\n0 rock 10\n"),
        LineError::DuplicateId
    );
    // A forged id cannot make the table large.
    assert_eq!(line_error("65535 brick 25\n"), LineError::Gap);
}

#[test]
fn numbers_are_plain_decimals_in_range() {
    for id in [
        "-1",
        "+0",
        "00",
        "0x0",
        "65536",
        "99999999999999",
        "a",
        "1.0",
    ] {
        assert_eq!(line_error(&format!("{id} brick 25\n")), LineError::BadId);
    }
    for gloss in ["-1", "+25", "025", "256", "2.5", "x"] {
        assert_eq!(
            line_error(&format!("0 brick {gloss}\n")),
            LineError::BadGloss,
            "{gloss}"
        );
    }
}

#[test]
fn names_that_could_reach_another_file_are_refused() {
    let names = [
        "..",
        "../evil",
        "..\\evil",
        "a/b",
        "c:evil",
        "/etc/passwd",
        "a.b",
        "brick_n",
        "brick_h",
        "brick_s",
        "_hidden",
        "Brick",
        "-a",
        "a-",
        "-",
        "con",
        "nul",
        "aux",
        "prn",
        "com1",
        "lpt9",
        "\u{e9}t\u{e9}",
        "a\u{0}b",
    ];
    for name in names {
        assert_eq!(
            line_error(&format!("0 {name} 10\n")),
            LineError::BadName,
            "{name:?}"
        );
    }
    let long = "a".repeat(49);
    assert_eq!(line_error(&format!("0 {long} 10\n")), LineError::BadName);
    assert!(parse(format!("0 {} 10\n", "a".repeat(48)).as_bytes()).is_ok());
    assert_eq!(line_error("0 retired 10\n"), LineError::BadName);
}

#[test]
fn a_name_can_be_used_once_but_a_retired_id_many_times() {
    assert_eq!(
        line_error("0 brick 25\n1 brick 90\n"),
        LineError::DuplicateName
    );
    let text = format!("{STOCK_LINES}5 retired\n6 retired\n");
    assert_eq!(parse(text.as_bytes()).unwrap().len(), 7);
}

#[test]
fn stock_ids_cannot_be_retired() {
    assert_eq!(line_error("0 retired\n"), LineError::RetiredStock);
    assert_eq!(
        line_error("0 a 1\n1 b 2\n2 c 3\n3 d 4\n4 retired\n"),
        LineError::RetiredStock
    );
}

#[test]
fn lines_must_have_the_right_fields() {
    for line in [
        "0",
        "0 brick",
        "0 brick 25 4 extra",
        "0 retired 25 4 x",
        "0 25",
    ] {
        assert_eq!(
            line_error(&format!("{line}\n")),
            LineError::BadFields,
            "{line}"
        );
    }
    // Comments only work on their own line.
    assert_eq!(line_error("0 brick 25 # shiny\n"), LineError::BadFields);
}

#[test]
fn a_line_may_end_with_a_relief_depth_and_old_lines_read_as_before() {
    let text = format!("{STOCK_LINES}5 brick 25 4\n6 floor 90\n7 rock 10 0\n8 retired\n");
    let entries = parse(text.as_bytes()).expect("parses");
    let depth = |i: usize| match &entries[i] {
        Entry::Material { depth, .. } => *depth,
        Entry::Retired => panic!("retired"),
    };
    assert_eq!((depth(5), depth(6), depth(7)), (Some(4), None, Some(0)));
    // Written back, the depths survive, and a line without one gets none.
    assert_eq!(text_of(&entries), text);
}

#[test]
fn a_depth_is_a_plain_number_within_the_most_and_only_a_material_has_one() {
    let bad = |text: &str| line_error(&format!("0 a 1\n{text}"));
    assert_eq!(bad("1 b 2 33\n"), LineError::BadDepth);
    assert_eq!(bad("1 b 2 -1\n"), LineError::BadDepth);
    assert_eq!(bad("1 b 2 04\n"), LineError::BadDepth);
    assert_eq!(bad("1 b 2 x\n"), LineError::BadDepth);
    assert_eq!(bad("1 b 2 3 4\n"), LineError::BadFields);
    assert_eq!(bad("1 retired 3\n"), LineError::BadName);
    assert!(parse(b"0 a 1 32\n").is_ok());
}

#[test]
fn changing_a_gloss_keeps_the_materials_depth() {
    let (text, id) = with_material("0 a 1 7\n1 b 2\n", "a", 9).expect("changes");
    assert_eq!((text.as_str(), id), ("0 a 9 7\n1 b 2\n", 0));
}

#[test]
fn line_numbers_count_every_line() {
    assert_eq!(
        parse(b"# one\n\n0 brick x\n"),
        Err(ManifestError::Line(3, LineError::BadGloss))
    );
}

#[test]
fn size_limits_hold_and_nothing_is_allocated_for_them() {
    assert_eq!(parse(b""), Err(ManifestError::Empty));
    assert_eq!(parse(b"# nothing\n\n"), Err(ManifestError::Empty));
    assert_eq!(parse(b"\xff\xfe0 brick 25\n"), Err(ManifestError::NotText));
    let long = format!("# {}\n0 brick 25\n", "x".repeat(MAX_LINE));
    assert_eq!(
        parse(long.as_bytes()),
        Err(ManifestError::Line(1, LineError::TooLong))
    );
    let huge = vec![b'#'; MAX_BYTES as usize + 1];
    assert_eq!(parse(&huge), Err(ManifestError::TooLarge));

    let mut many = STOCK_LINES.to_string();
    for id in 5..=MAX_MATERIALS {
        many.push_str(&format!("{id} retired\n"));
    }
    assert_eq!(parse(many.as_bytes()), Err(ManifestError::TooMany));
}

#[test]
fn every_bit_flip_and_truncation_is_an_answer_not_a_panic() {
    let good = b"# stock\n0 brick 25\n1 floor 90\n2 lamp 7\n3 metal 200\n4 r 1\n5 retired\n";
    for at in 0..good.len() {
        parse(&good[..at]).ok();
        for bit in 0..8 {
            let mut bad = good.to_vec();
            bad[at] ^= 1 << bit;
            if let Ok(entries) = parse(&bad) {
                for e in entries {
                    if let Entry::Material { name, .. } = e {
                        assert!(name_is_valid(&name), "{name:?}");
                    }
                }
            }
        }
    }
}
