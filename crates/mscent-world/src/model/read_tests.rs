use super::*;
use crate::voxel::CellSize;

const STOCK: [(&str, &str); 3] = [
    (
        "sentry",
        include_str!("../../../../assets/enemies/sentry.msb"),
    ),
    ("dart", include_str!("../../../../assets/enemies/dart.msb")),
    (
        "mantis",
        include_str!("../../../../assets/enemies/mantis.msb"),
    ),
];

const HEAD: &str = "miniscent-blueprint 1\nname a\nclass drone\nskin hull metal\n";

/// The error of a blueprint made of the usual start and `body`.
fn why(body: &str) -> String {
    match read(format!("{HEAD}{body}").as_bytes()) {
        Ok(_) => panic!("took: {body}"),
        Err(ModelError::Parse { why, .. }) => why,
        Err(e) => panic!("{e}"),
    }
}

fn takes(body: &str) -> Blueprint {
    read(format!("{HEAD}{body}").as_bytes()).unwrap()
}

#[test]
fn every_stock_blueprint_reads_and_writes_back_the_same() {
    for (name, text) in STOCK {
        let first = read(text.as_bytes()).unwrap();
        assert_eq!(first.name, name);
        let written = write(&first);
        let second = read(written.as_bytes()).unwrap();
        assert_eq!(first, second);
        assert_eq!(written, write(&second), "writing settles at once");
    }
}

#[test]
fn numbers_survive_exactly() {
    let b =
        takes("part p\nadd sphere 0.1 -0.30000000000000004 1e-3 1.7 hull\nhit 0.1 0.2 0.3 0.7\n");
    let again = read(write(&b).as_bytes()).unwrap();
    assert_eq!(b, again);
    assert_eq!(again.hits[0].centre.x.to_bits(), 0.1f64.to_bits());
}

#[test]
fn what_a_file_leaves_out_takes_its_default() {
    let b = takes("part p\nadd sphere 0 0 0 2 hull\n");
    assert_eq!(b.scale, DEFAULT_SCALE);
    assert_eq!(b.plan, None);
    let p = &b.parts[0];
    assert_eq!(p.cell, CellSize::Half);
    assert_eq!(p.motion, Motion::Still);
}

#[test]
fn comments_and_blank_lines_mean_nothing() {
    let a = takes("part p\nadd sphere 0 0 0 2 hull\n");
    let b = takes("# one\n\npart p   # two\n  add sphere 0 0 0 2 hull\n\n");
    assert_eq!(a, b);
}

#[test]
fn a_cut_with_no_role_looks_like_the_first_skin() {
    let b = takes("part p\nsolid cube 0 0 0 2 2 2 hull\ncut cube 2 0 0 1 1 1\n");
    assert!(matches!(b.parts[0].pieces[1], Piece::Cut { role: 0, .. }));
}

#[test]
fn a_header_and_required_lines_are_required() {
    let line0 = |bytes: &[u8]| matches!(read(bytes), Err(ModelError::Parse { line: 0, .. }));
    assert!(line0(b""));
    assert!(line0(b"miniscent-blueprint 2\n"));
    assert!(line0(&[0xff, 0xfe]));
    assert!(line0(
        b"miniscent-blueprint 1\nclass drone\nskin h m\npart p\nadd sphere 0 0 0 1 h\n"
    ));
    assert!(line0(
        b"miniscent-blueprint 1\nname a\nskin h m\npart p\nadd sphere 0 0 0 1 h\n"
    ));
    assert!(line0(HEAD.as_bytes()), "no parts");
    assert!(
        line0(format!("{HEAD}part p\n").as_bytes()),
        "a part with no pieces"
    );
}

#[test]
fn numbers_out_of_range_are_errors() {
    for bad in ["NaN", "inf", "-inf", "1e300", "257", "x", ""] {
        why(&format!("part p\nadd sphere {bad} 0 0 2 hull\n"));
    }
    why("part p\nadd sphere 0 0 0 0 hull\n");
    why("part p\nadd sphere 0 0 0 -1 hull\n");
    why("scale 0\n");
    why("scale 17\n");
    why("hit 0 0 0 0\n");
    why("part p motion=spin:65\nadd sphere 0 0 0 1 hull\n");
    why("part p axis=0,0,0\nadd sphere 0 0 0 1 hull\n");
    why("part p cell=0.3\nadd sphere 0 0 0 1 hull\n");
}

#[test]
fn shapes_that_make_no_solid_are_errors() {
    why("part p\nsolid cube 0 0 0 0 1 1 hull\n");
    why("part p\nsolid ngon 1 1 1  1 1 1  1 1  6 hull\n");
    why("part p\nsolid ngon 0 0 0  4 0 0  1 1  2 hull\n");
    why("part p\nsolid ngon 0 0 0  4 0 0  1 1  33 hull\n");
    why("part p\nsolid ngon 0 0 0  4 0 0  1 1  4.5 hull\n");
    why("part p\nsolid plate 1  0 0 0  1 0 0  2 0 0 hull\n");
    why("part p\nsolid plate 1  0 0 0  1 0 0 hull\n");
    why("part p\nsolid plate 0.001  0 0 0  8 0 0  8 8 0 hull\n");
    why("part p\nadd round_cone 0 0 0 hull\n");
    why("part p\nadd teapot 0 0 0 1 hull\n");
}

#[test]
fn names_and_roles_must_be_good() {
    why("skin BAD metal\n");
    why("skin hull metal\n");
    why("skin ../up metal\n");
    why(&format!("skin {} metal\n", "a".repeat(MAX_NAME + 1)));
    why("part p\nadd sphere 0 0 0 1 nobody\n");
    why("part p\nsolid cube 0 0 0 1 1 1 nobody\n");
    why("part p\nadd sphere 0 0 0 1 hull\npart p\nadd sphere 0 0 0 1 hull\n");
    why("add sphere 0 0 0 1 hull\n");
    why("nonsense\n");
}

#[test]
fn each_limit_holds() {
    let parts: String = (0..=MAX_PARTS)
        .map(|i| format!("part p{i}\nadd sphere 0 0 0 1 hull\n"))
        .collect();
    assert_eq!(why(&parts), "too many parts");
    let hits = "hit 0 0 0 1\n".repeat(MAX_HITS + 1);
    assert_eq!(why(&hits), "too many hit spheres");
    let skins: String = (0..=MAX_ROLES)
        .map(|i| format!("skin r{i} metal\n"))
        .collect();
    assert_eq!(why(&skins), "too many skins");
    let pieces = "carve sphere 0 0 0 1\n".repeat(MAX_PIECES + 1);
    assert_eq!(why(&format!("part p\n{pieces}")), "too many pieces");
    let solids = "solid cube 0 0 0 1 1 1 hull\n".repeat(MAX_CUT_BRUSHES);
    assert!(why(&format!("part p\n{solids}cut cube 0 0 0 1 1 1\n")).contains("cuts"));
    let big = format!("{HEAD}{}", "#".repeat(MAX_FILE));
    assert!(read(big.as_bytes()).is_err());
}

#[test]
fn a_box_past_a_volume_or_the_chunk_limit_is_an_error() {
    assert!(why("part p cell=0.125\nadd sphere 0 0 0 200 hull\n").contains("too long"));
    let spread: String = (0..8)
        .map(|i| format!("add sphere {} 0 0 28 hull\n", i * 4))
        .collect();
    let many: String = (0..8)
        .map(|i| format!("part p{i} cell=0.5\n{spread}"))
        .collect();
    assert!(why(&many).contains("chunks"));
}
