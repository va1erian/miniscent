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
    }
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
    for line in ["0", "0 brick", "0 brick 25 extra", "0 retired 25 x", "0 25"] {
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
