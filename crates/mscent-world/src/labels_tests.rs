use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::format::{self, FormatError, MAGIC};
use crate::labels::{EntityLabel, LabelError, MAX_GROUP_BRUSHES, MAX_TAGS, MoverGroup};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};

fn cuboid(x: f64, op: Op, order: u32) -> Brush {
    let (lo, hi) = (Vec3d::new(x, 0.0, 0.0), Vec3d::new(x + 1.0, 1.0, 1.0));
    Brush::cuboid(lo, hi, op, order).unwrap()
}

fn marker(x: f32) -> Entity {
    Entity::Marker {
        pos: Vec3::new(x, 0.0, 0.0),
        yaw: 0.5,
    }
}

fn label(entity: u32, name: &str, tags: &[&str]) -> EntityLabel {
    EntityLabel {
        entity,
        name: (!name.is_empty()).then(|| name.to_string()),
        tags: tags.iter().map(|t| t.to_string()).collect(),
    }
}

fn group(name: &str, brushes: &[u32]) -> MoverGroup {
    MoverGroup {
        name: name.to_string(),
        tags: Vec::new(),
        brushes: brushes.to_vec(),
    }
}

/// A subtract room, two door brushes in a group, a zone and two markers.
fn sample() -> Level {
    Level {
        volumes: Vec::new(),
        brushes: vec![
            cuboid(0.0, Op::Subtract, 0),
            cuboid(2.0, Op::Add, 1),
            cuboid(4.0, Op::Add, 2),
            cuboid(6.0, Op::Add, 3),
        ],
        entities: vec![
            marker(1.0),
            Entity::Zone {
                pos: Vec3::new(2.0, 2.0, 2.0),
                half: Vec3::new(1.0, 2.0, 3.0),
            },
            marker(3.0),
        ],
        labels: vec![
            label(0, "trap_hinge", &[]),
            label(1, "hall", &["alarm", "east"]),
            label(2, "", &["vault_switch"]),
        ],
        mover_groups: vec![MoverGroup {
            tags: vec!["doors".to_string()],
            ..group("vault_door", &[1, 2])
        }],
    }
}

fn check(edit: impl FnOnce(&mut Level)) -> Result<(), LabelError> {
    let mut level = sample();
    edit(&mut level);
    level.check_labels()
}

#[test]
fn sample_is_valid_and_round_trips_byte_exactly() {
    let level = sample();
    assert_eq!(level.check_labels(), Ok(()));
    let bytes = format::write(&level);
    let back = format::read(&bytes).unwrap();
    assert_eq!(back, level);
    assert_eq!(format::write(&back), bytes);
}

#[test]
fn a_level_without_labels_writes_no_labels_section() {
    let level = Level {
        labels: Vec::new(),
        mover_groups: Vec::new(),
        ..sample()
    };
    let bytes = format::write(&level);
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 2);
    assert_eq!(format::read(&bytes).unwrap(), level);
}

#[test]
fn bad_names_are_rejected() {
    for bad in ["", "Upper", "has space", "dash-ed", "é", &"x".repeat(33)] {
        let name = bad.to_string();
        let result = check(|l| l.mover_groups[0].name = name);
        assert_eq!(result, Err(LabelError::BadName), "{bad:?}");
    }
    assert_eq!(check(|l| l.mover_groups[0].name = "x".repeat(32)), Ok(()));
    let bad_tag = check(|l| l.labels[1].tags[0] = "No".to_string());
    assert_eq!(bad_tag, Err(LabelError::BadName));
}

#[test]
fn each_inconsistency_has_its_error() {
    use LabelError as E;
    assert_eq!(check(|l| l.labels[0].entity = 3), Err(E::BadIndex));
    assert_eq!(check(|l| l.labels[1].entity = 0), Err(E::DuplicateLabel));
    assert_eq!(check(|l| l.labels[0].name = None), Err(E::EmptyLabel));
    let dup = check(|l| l.labels[1].name = Some("trap_hinge".to_string()));
    assert_eq!(dup, Err(E::DuplicateName));
    let dup = check(|l| l.mover_groups[0].name = "hall".to_string());
    assert_eq!(dup, Err(E::DuplicateName));
    let dup = check(|l| l.labels[1].tags[1] = "alarm".to_string());
    assert_eq!(dup, Err(E::DuplicateTag));
    let many = check(|l| l.labels[1].tags = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect());
    assert_eq!(many, Err(E::TooMany));
    assert_eq!(
        check(|l| l.mover_groups[0].brushes.clear()),
        Err(E::EmptyGroup)
    );
    assert_eq!(
        check(|l| l.mover_groups[0].brushes = vec![9]),
        Err(E::BadIndex)
    );
    assert_eq!(
        check(|l| l.mover_groups[0].brushes = vec![0]),
        Err(E::MoverNotAdd)
    );
    assert_eq!(
        check(|l| l.mover_groups[0].brushes = vec![1, 1]),
        Err(E::BrushInTwoGroups)
    );
    let second = check(|l| l.mover_groups.push(group("other", &[2])));
    assert_eq!(second, Err(E::BrushInTwoGroups));
    let over = check(|l| l.mover_groups[0].brushes = vec![1; MAX_GROUP_BRUSHES + 1]);
    assert_eq!(over, Err(E::TooMany));
}

#[test]
fn a_file_with_bad_labels_is_rejected_whatever_the_section_order() {
    let mut level = sample();
    level.labels[0].entity = 99;
    let bytes = format::write(&level);
    let expected = Err(FormatError::Labels(LabelError::BadIndex));
    assert_eq!(format::read(&bytes), expected);
}

#[test]
fn static_brushes_leave_out_movers_and_keep_the_rest() {
    let level = sample();
    let kept = level.static_brushes();
    let orders: Vec<u32> = kept.iter().map(Brush::order).collect();
    assert_eq!(orders, [0, 3]);
    let plain = Level::default();
    assert!(matches!(
        plain.static_brushes(),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn static_brushes_survive_a_hostile_level_built_in_code() {
    let mut level = sample();
    level.mover_groups[0].brushes = vec![u32::MAX, 1];
    assert_eq!(level.static_brushes().len(), 3);
}

#[test]
fn zone_and_marker_values_are_validated() {
    let p = Vec3::new(0.0, 0.0, 0.0);
    let zone = |half| Entity::Zone { pos: p, half };
    assert!(zone(Vec3::new(1.0, 1.0, 1.0)).is_valid());
    assert!(!zone(Vec3::new(0.0, 1.0, 1.0)).is_valid());
    assert!(!zone(Vec3::new(1.0, -1.0, 1.0)).is_valid());
    assert!(!zone(Vec3::new(1.0, 1.0, f32::NAN)).is_valid());
    assert!(
        !Entity::Marker {
            pos: p,
            yaw: f32::INFINITY
        }
        .is_valid()
    );
    assert!(
        !Entity::Marker {
            pos: Vec3::new(f32::NAN, 0.0, 0.0),
            yaw: 0.0
        }
        .is_valid()
    );
}

#[test]
fn version_2_files_still_load_and_ignore_a_labels_section() {
    let mut bytes = format::write(&sample());
    bytes[4..6].copy_from_slice(&2u16.to_le_bytes());
    let back = format::read(&bytes).unwrap();
    assert!(back.labels.is_empty() && back.mover_groups.is_empty());
    assert_eq!(back.brushes, sample().brushes);
}

#[test]
fn shipped_levels_still_load() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/levels/");
    for file in ["showcase.msl", "corridor.msl"] {
        let bytes = std::fs::read(format!("{root}{file}")).unwrap();
        assert_eq!(bytes[..4], MAGIC);
        let level = format::read(&bytes).unwrap();
        assert!(!level.brushes.is_empty(), "{file}");
    }
}

#[test]
fn every_truncation_is_an_error() {
    let bytes = format::write(&sample());
    for len in 0..bytes.len() {
        assert!(format::read(&bytes[..len]).is_err(), "length {len}");
    }
}

#[test]
fn bit_flips_never_panic_and_never_yield_an_inconsistent_level() {
    let bytes = format::write(&sample());
    let mut rng = Rng::new(0x1abe1);
    let mut loaded = 0;
    for _ in 0..200_000 {
        let mut b = bytes.clone();
        for _ in 0..(rng.next_u64() % 4 + 1) {
            let at = rng.next_u64() as usize % b.len();
            b[at] ^= 1 << (rng.next_u64() % 8);
        }
        if let Ok(level) = format::read(&b) {
            loaded += 1;
            assert_eq!(level.check_labels(), Ok(()));
            let _ = level.static_brushes();
        }
    }
    assert!(loaded > 0, "the fuzz never produced a loadable level");
}

#[test]
fn group_brush_count_cannot_force_a_large_allocation() {
    let good = format::write(&sample());
    // The group's brush count sits just before its two indices, at the end.
    let at = good.len() - 12;
    assert_eq!(good[at..at + 4], 2u32.to_le_bytes());
    let mut b = good.clone();
    b[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(format::read(&b), Err(FormatError::TooMany));
}
