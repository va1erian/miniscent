use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::labels::{EntityLabel, MoverGroup};
use crate::level_text::{from_text, to_text};
use crate::texinfo::TexInfo;
use crate::{format, showcase_level};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};

const HAND_WRITTEN: &str = "miniscent-level 1
# A room with a door.
box subtract 0 1  -20 -20 -20  20 20 20
box add 1 2  5 -3 -3  6 3 3   # the door
player 0 0 0  1.5
light 0 0 8  1 0.5 0.25  16 1.25
enemy 5 5 0 1  name guard  tag guards  tag east
goal 8 8 0 1.5
world 0 0 0  4 26
marker 3 0 0 0  name hinge
zone 0 10 0  2 2 2  name hall
mover door tag doors brushes 1
";

#[test]
fn a_hand_written_level_reads_as_written() {
    let level = from_text(HAND_WRITTEN).unwrap();
    assert_eq!(level.brushes.len(), 2);
    assert_eq!(level.brushes[1].material(), 2);
    assert_eq!(level.entities.len(), 7);
    assert_eq!(
        level.entities[2],
        Entity::Enemy {
            pos: Vec3::new(5.0, 5.0, 0.0),
            kind: 1
        }
    );
    assert_eq!(
        level.labels[0],
        EntityLabel {
            entity: 2,
            name: Some("guard".to_string()),
            tags: vec!["guards".to_string(), "east".to_string()],
        }
    );
    assert_eq!(level.mover_groups[0].brushes, [1]);
    assert_eq!(level.mover_groups[0].tags, ["doors"]);
}

#[test]
fn text_round_trips_to_an_equal_level_and_the_same_text() {
    let level = from_text(HAND_WRITTEN).unwrap();
    let text = to_text(&level);
    assert_eq!(from_text(&text).unwrap(), level);
    assert_eq!(to_text(&from_text(&text).unwrap()), text);
}

#[test]
fn the_showcase_survives_with_its_texture_mappings() {
    let level = showcase_level();
    let back = from_text(&to_text(&level)).unwrap();
    assert_eq!(back, level);
    assert_eq!(format::write(&back), format::write(&level));
}

#[test]
fn a_brush_that_is_not_a_plain_box_keeps_every_plane() {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let d = |x, y, z, dist| mscent_math::DPlane::new(Vec3d::new(x, y, z), dist);
    let planes = [
        d(1.0, 0.0, 0.0, 4.0),
        d(-1.0, 0.0, 0.0, 4.0),
        d(0.0, 1.0, 0.0, 4.0),
        d(0.0, -1.0, 0.0, 4.0),
        d(0.0, 0.0, -1.0, 4.0),
        d(-s, 0.0, s, 0.5),
    ];
    let mut ramp = Brush::new(&planes, Op::Add, 3).unwrap();
    let mut info = ramp.texinfos()[1];
    info.u_shift = 0.125;
    info.lock_to_world = true;
    assert!(ramp.set_texinfo(1, info));
    // A box whose faces are not all the same material is not a `box` line either.
    let mut mixed = Brush::cuboid(Vec3d::ZERO, Vec3d::new(1.0, 1.0, 1.0), Op::Add, 4).unwrap();
    let mut face = mixed.texinfos()[0];
    face.material = 9;
    assert!(mixed.set_texinfo(0, face));
    let level = Level {
        brushes: vec![ramp, mixed],
        ..Level::default()
    };
    let text = to_text(&level);
    assert_eq!(text.matches("brush ").count(), 2, "{text}");
    assert_eq!(from_text(&text).unwrap(), level);
}

#[test]
fn numbers_keep_every_bit() {
    let level = Level {
        entities: vec![Entity::Marker {
            pos: Vec3::new(0.1, 1.0e-7, 123_456.79),
            yaw: core::f32::consts::PI,
        }],
        labels: vec![EntityLabel {
            entity: 0,
            name: Some("m".to_string()),
            tags: Vec::new(),
        }],
        mover_groups: Vec::<MoverGroup>::new(),
        ..Level::default()
    };
    assert_eq!(from_text(&to_text(&level)).unwrap(), level);
}

#[test]
fn each_mistake_names_its_line() {
    let cases: &[(&str, usize, &str)] = &[
        ("not a level", 1, "first line"),
        ("miniscent-level 1\nfrog 1 2 3", 2, "unknown line type"),
        ("miniscent-level 1\n\nbox add 0 0 1 2", 3, "end of the line"),
        ("miniscent-level 1\nbox add 0 0 a 0 0 1 1 1", 2, "not 'a'"),
        (
            "miniscent-level 1\nbox nope 0 0 0 0 0 1 1 1",
            2,
            "add or subtract",
        ),
        ("miniscent-level 1\nbox add 0 0 0 0 0 0 0 0", 2, "bad box"),
        (
            "miniscent-level 1\nbox add 0 0 0 0 0 1 1 1 extra",
            2,
            "unexpected 'extra'",
        ),
        ("miniscent-level 1\nplayer nan 0 0 0", 2, "must be finite"),
        ("miniscent-level 1\nplayer inf 0 0 0", 2, "must be finite"),
        ("miniscent-level 1\nzone 0 0 0 -1 1 1", 2, "out of range"),
        (
            "miniscent-level 1\nmarker 0 0 0 0 color red",
            2,
            "name or tag",
        ),
        ("miniscent-level 1\nmover door", 2, "tag or brushes"),
        (
            "miniscent-level 1\nmover door brushes x",
            2,
            "bad brush index",
        ),
        ("miniscent-level 1\nbrush add 0 0 99999999 0", 2, "too many"),
    ];
    for (text, line, wanted) in cases {
        let e = from_text(text).unwrap_err();
        assert_eq!(e.line, *line, "{text}: {e}");
        assert!(e.msg.contains(wanted), "{text}: {e}");
    }
}

#[test]
fn a_level_that_reads_but_is_inconsistent_is_refused_as_a_whole() {
    for text in [
        "miniscent-level 1\nmover door brushes 5",
        "miniscent-level 1\nmarker 0 0 0 0 name a\nmarker 0 0 0 0 name a",
        "miniscent-level 1\nmarker 0 0 0 0 name Bad",
        "miniscent-level 1\nbox subtract 0 0 0 0 0 1 1 1\nmover door brushes 0",
    ] {
        let e = from_text(text).unwrap_err();
        assert_eq!(e.line, 0, "{text}: {e}");
    }
}

#[test]
fn hostile_text_is_refused_quickly_and_quotes_little() {
    let long = "x".repeat(1 << 20);
    let e = from_text(&format!("miniscent-level 1\n{long}")).unwrap_err();
    assert!(e.msg.len() < 100, "{}", e.msg.len());
    let e = from_text(&"x".repeat(format::MAX_FILE_BYTES + 1)).unwrap_err();
    assert_eq!(e.line, 0);
    let many = format!("miniscent-level 1\n{}", "marker 0 0 0 0\n".repeat(70_000));
    assert!(from_text(&many).unwrap_err().msg.contains("too many"));
}

#[test]
fn seeded_mutations_never_panic_and_anything_accepted_is_consistent() {
    let base = to_text(&from_text(HAND_WRITTEN).unwrap());
    let mut rng = Rng::new(0x7e87);
    const CHARS: &[u8] = b"0123456789 .-eEaxn\n#";
    let mut accepted = 0;
    for _ in 0..40_000 {
        let mut b = base.clone().into_bytes();
        for _ in 0..(rng.below(4) + 1) {
            let at = rng.below(b.len() as u32) as usize;
            b[at] = CHARS[rng.below(CHARS.len() as u32) as usize];
        }
        let Ok(text) = String::from_utf8(b) else {
            continue;
        };
        if let Ok(level) = from_text(&text) {
            accepted += 1;
            assert_eq!(level.check_labels(), Ok(()));
            assert!(level.entities.iter().all(Entity::is_valid));
            assert_eq!(from_text(&to_text(&level)).unwrap(), level);
        }
    }
    assert!(accepted > 0, "the fuzz never produced a level");
}

#[test]
fn vista_lines_round_trip_in_decimal_and_hex() {
    let text = "miniscent-level 1
box subtract 0 65520  -20 -20 -20  20 20 20
vista 1 2 3  4 5 6  2 1 0.0625  0 0 0.5  1.5 -2  0 100000  0x8090a0  name sky
vista 0 0 0  0 0 0  0 0 0  0 0 0  0 0  1 2  12648430
";
    let level = from_text(text).unwrap();
    assert_eq!(level.brushes[0].material(), 65520);
    assert_eq!(level.vista_layers().len(), 2);
    let Entity::VistaLayer {
        fog_color, slot, ..
    } = level.entities[1]
    else {
        panic!("not a vista layer");
    };
    assert_eq!((slot, fog_color), (0, 0x00C0_FFEE));
    let again = to_text(&level);
    assert!(again.contains("0x8090a0"));
    assert_eq!(from_text(&again).unwrap(), level);
    assert_eq!(to_text(&from_text(&again).unwrap()), again);
}

#[test]
fn bad_vista_lines_are_refused() {
    let head = "miniscent-level 1\n";
    for line in [
        "vista 0 0 0  0 0 0  8 0 0.5  0 0 0  0 0  0 10  0",
        "vista 0 0 0  0 0 0  0 4 0.5  0 0 0  0 0  0 10  0",
        "vista 0 0 0  0 0 0  0 0 1.5  0 0 0  0 0  0 10  0",
        "vista 0 0 0  0 0 0  0 0 0.5  0 0 0  0 0  5 5  0",
        "vista 0 0 0  0 0 0  0 0 0.5  0 0 0  0 0  0 10  0xzz",
        "vista 0 0 0  0 0 0  0 0 0.5  0 0 0  0 0  0 10  4294967296",
        "vista 0 0 0  0 0 0  0 0 0.5  0 0 0  0 0  0 10",
    ] {
        assert!(from_text(&format!("{head}{line}\n")).is_err(), "{line}");
    }
}

#[test]
fn viewport_materials_survive_in_box_and_brush_lines() {
    let one = |slot: u16| {
        let (lo, hi) = (Vec3d::new(0.0, 0.0, 0.0), Vec3d::new(1.0, 1.0, 1.0));
        let b = Brush::cuboid(lo, hi, Op::Add, u32::from(slot)).unwrap();
        b.with_material(0xFFF0 + slot)
    };
    let level = Level {
        brushes: (0..8).map(one).collect(),
        ..Default::default()
    };
    let text = to_text(&level);
    assert!(text.contains(" 65527 "));
    assert_eq!(from_text(&text).unwrap(), level);

    // A face of a brush that is not a box carries its own number.
    let base = &from_text("miniscent-level 1\nbox add 0 0  0 0 0  1 1 1\n")
        .unwrap()
        .brushes[0];
    let mut planes = base.planes().to_vec();
    planes.push(mscent_math::DPlane::new(Vec3d::new(0.6, 0.0, 0.8), 1.2));
    let mut infos = base.texinfos().to_vec();
    infos.push(TexInfo {
        material: 65520,
        ..TexInfo::default_for_normal(Vec3d::new(0.6, 0.0, 0.8))
    });
    let cut = Brush::new_textured(&planes, &infos, Op::Add, 0).unwrap();
    let level = Level {
        brushes: vec![cut],
        ..Default::default()
    };
    let text = to_text(&level);
    assert!(text.starts_with("miniscent-level 1\nbrush"));
    assert_eq!(from_text(&text).unwrap(), level);
}

#[test]
fn a_scale_line_reads_writes_and_reads_back() {
    let text =
        "miniscent-level 1\nbox subtract 0 1  -8 -8 -8  8 8 8\nplayer 0 0 0 0\nscale 1 2 3  5\n";
    let level = from_text(text).unwrap();
    assert_eq!(level.ship_scale(), Some(5.0));
    assert!(to_text(&level).contains("scale 1 2 3 5"));
    let again = crate::format::read(&crate::format::write(&level)).unwrap();
    assert_eq!(again, level);
}

#[test]
fn a_ship_size_out_of_range_is_refused() {
    for bad in ["0", "-2", "100", "nan", "inf"] {
        let text = format!("miniscent-level 1\nplayer 0 0 0 0\nscale 0 0 0  {bad}\n");
        assert!(from_text(&text).is_err(), "{bad} was taken");
    }
}

#[test]
fn a_scale_in_an_old_file_is_refused() {
    let level = from_text("miniscent-level 1\nplayer 0 0 0 0\nscale 0 0 0 2\n").unwrap();
    let mut bytes = crate::format::write(&level);
    // The version follows the four magic bytes.
    bytes[4..6].copy_from_slice(&7u16.to_le_bytes());
    assert_eq!(
        crate::format::read(&bytes),
        Err(crate::format::FormatError::BadEntityTag(9))
    );
}
