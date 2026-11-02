//! The `mountains` line and the `x` scale of a `vista` line, read as untrusted text.

use crate::entity::{Entity, Level};
use crate::level_text::{from_text, to_text};
use crate::{format, voxel};

const HEAD: &str = "miniscent-level 1\nbox subtract 0 1  -20 -20 -20  20 20 20\n";
const LINE: &str = "mountains 3 4  0 100 0  128 228 60  1 40 7";

fn text(lines: &[&str]) -> String {
    format!("{HEAD}{}\n", lines.join("\n"))
}

#[test]
fn a_mountains_line_builds_a_volume_that_packs_and_reloads() {
    let level = from_text(&text(&[LINE])).unwrap();
    assert_eq!(level.volumes.len(), 1);
    assert_eq!(level.volumes[0].order(), 3);
    assert_eq!(level.volumes[0].cell(), voxel::CellSize::One);
    // The same text builds the same bytes.
    let again = from_text(&text(&[LINE])).unwrap();
    assert_eq!(format::write(&level), format::write(&again));
    assert_eq!(format::read(&format::write(&level)).unwrap(), level);
    let other = LINE.replace(" 7", " 8");
    let changed = from_text(&text(&[&other])).unwrap();
    assert_ne!(format::write(&level), format::write(&changed));
}

#[test]
fn text_writing_leaves_volumes_out_so_dumping_must_refuse_them() {
    let level = from_text(&text(&[LINE])).unwrap();
    assert!(!to_text(&level).contains("mountains"));
}

#[test]
fn hostile_mountains_lines_are_errors_with_their_line() {
    let bad = [
        "mountains 3 4  0 0 0  1e300 1 1  1 40 7",
        "mountains 3 4  0 0 0  nan 100 60  1 40 7",
        "mountains 3 4  0 0 0  128 128 60  0 40 7",
        "mountains 3 4  0 0 0  128 128 60  3 40 7",
        "mountains 3 4  0 0 0  128 128 60  1 inf 7",
        "mountains 3 4  0 0 0  128 128 60  1 -4 7",
        "mountains 3 4  0 0 0  9000 128 60  1 40 7",
        "mountains 3 4  0 0 0  128 128 60  1 90 7",
        "mountains 3 4  128 0 0  0 128 60  1 40 7",
        "mountains 0 4  0 0 0  128 128 60  1 40 7",
        "mountains 3 300  0 0 0  128 128 60  1 40 7",
        "mountains 3 4  0 0 0  128 128 60  1 40 7 extra",
        "mountains 3 4  0 0 0  128 128 60  1 40",
        "mountains 3 4  -99999 0 0  128 128 60  1 40 7",
    ];
    for line in bad {
        let err = from_text(&text(&[line])).unwrap_err();
        assert_eq!(err.line, 3, "{line}: {err}");
    }
}

#[test]
fn a_range_may_not_overlap_another_or_share_its_order() {
    let overlap = "mountains 4 4  64 100 0  192 228 60  1 40 7";
    assert!(from_text(&text(&[LINE, overlap])).is_err());
    let same_order = "mountains 3 4  300 100 0  428 228 60  1 40 7";
    assert!(from_text(&text(&[LINE, same_order])).is_err());
    let apart = "mountains 4 4  300 100 0  428 228 60  1 40 7";
    assert_eq!(from_text(&text(&[LINE, apart])).unwrap().volumes.len(), 2);
}

#[test]
fn many_big_ranges_stop_at_the_level_budget_without_building_them_all() {
    let big = |i: u32| {
        format!(
            "mountains {} 4  {} 100 0  {} 356 60  1 40 7",
            10 + i,
            i * 300,
            i * 300 + 256
        )
    };
    let lines: Vec<String> = (0..40).map(big).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let started = std::time::Instant::now();
    let _ = from_text(&text(&refs));
    assert!(started.elapsed().as_secs() < 60);
}

#[test]
fn a_vista_scale_reads_as_its_inverse() {
    let vista = |p: &str| format!("vista 0 0 0  0 0 0  0 0 {p}  0 0 0  0 0  0 100000  0");
    let parallax = |p: &str| {
        let level: Level = from_text(&text(&[&vista(p)])).unwrap();
        match level.entities[0] {
            Entity::VistaLayer { parallax, .. } => parallax,
            _ => panic!("not a layer"),
        }
    };
    assert_eq!(parallax("x16"), 0.0625);
    assert_eq!(parallax("x1"), 1.0);
    assert_eq!(parallax("x2.5"), 0.4);
    assert_eq!(parallax("0.0625"), 0.0625);
    for bad in [
        "x0", "x0.5", "x", "x-4", "xnan", "xinf", "x1e999", "xx4", "X16",
    ] {
        assert!(from_text(&text(&[&vista(bad)])).is_err(), "{bad}");
    }
    // Dumped as the parallax, which reads back to the same bits.
    let level = from_text(&text(&[&vista("x3")])).unwrap();
    let dumped = to_text(&level);
    assert!(!dumped.contains(" x3 "));
    assert_eq!(from_text(&dumped).unwrap(), level);
}
