use super::*;

fn ascii_art(ch: char) -> Vec<String> {
    glyph(ch)
        .iter()
        .map(|row| {
            (0..8)
                .map(|b| if row >> b & 1 == 1 { '#' } else { '.' })
                .collect()
        })
        .collect()
}

#[test]
fn capital_a_has_the_expected_shape() {
    assert_eq!(
        ascii_art('A'),
        [
            "..##....", ".####...", "##..##..", "##..##..", "######..", "##..##..", "##..##..",
            "........"
        ]
    );
}

#[test]
fn every_printable_glyph_is_inked_except_space() {
    for code in 0x21u8..=0x7e {
        let g = glyph(char::from(code));
        assert!(g.iter().any(|&r| r != 0), "glyph {code:#x} is blank");
    }
    assert_eq!(glyph(' '), &[0u8; 8]);
}

#[test]
fn glyphs_are_distinct() {
    for a in 0x21u8..=0x7e {
        for b in (a + 1)..=0x7e {
            assert_ne!(
                glyph(char::from(a)),
                glyph(char::from(b)),
                "{a:#x} vs {b:#x}"
            );
        }
    }
}

#[test]
fn unsupported_characters_fall_back_to_question_mark() {
    assert_eq!(glyph('\u{e9}'), glyph('?'));
    assert_eq!(glyph('\n'), glyph('?'));
    assert_eq!(glyph('\u{1f600}'), glyph('?'));
    assert_eq!(glyph('\u{7f}'), glyph('?'));
}
