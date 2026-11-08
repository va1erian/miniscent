//! Enemy kinds in a level (M19.1): the stock table, both file forms, and
//! everything a hostile file can do to them.

use mscent_math::Vec3;
use mscent_math::rng::Rng;

use crate::entity::{Entity, Level};
use crate::format::{self, FormatError};
use crate::kinds::{
    Kind, KindError, KindStats, MAX_KIND_NAME, MAX_LEVEL_KINDS, MAX_LEVEL_MODEL_NS, stock_kinds,
};
use crate::level_text::{from_text, to_text};
use crate::model::{self, MAX_MODEL_NS, estimated_ns};

const MANTIS: &str = include_str!("../../../assets/enemies/mantis.msb");

fn kind(name: &str, text: &str) -> Kind {
    Kind {
        name: name.to_string(),
        stats: KindStats::HUNTER,
        blueprint: model::read(text.as_bytes()).unwrap(),
    }
}

fn enemy(kind: u8) -> Entity {
    Entity::Enemy {
        pos: Vec3::new(1.0, 2.0, 3.0),
        kind,
    }
}

/// The stock pair, then a mantis, with an enemy of each.
fn three_kinds() -> Level {
    let mut kinds = stock_kinds();
    kinds.push(kind("mantis", MANTIS));
    Level {
        entities: vec![enemy(0), enemy(1), enemy(2)],
        kinds,
        ..Level::default()
    }
}

fn refused(level: &Level) -> Result<Level, FormatError> {
    format::read(&format::write(level))
}

#[test]
fn stock_kinds_read() {
    let stock = stock_kinds();
    let names: Vec<&str> = stock.iter().map(|k| k.name.as_str()).collect();
    assert_eq!(names, ["drone", "hunter"]);
    // An unreadable asset would leave a design with no parts.
    assert!(stock.iter().all(|k| !k.blueprint.parts.is_empty()));
    assert_eq!(stock[0].blueprint.name, "sentry");
    assert_eq!(stock[1].blueprint.name, "dart");
    assert!(stock.iter().all(|k| k.stats.is_valid()));
}

#[test]
fn a_level_without_kinds_plays_the_stock_table() {
    let level = Level {
        entities: vec![enemy(0), enemy(1)],
        ..Level::default()
    };
    let back = refused(&level).unwrap();
    assert!(back.kinds.is_empty());
    assert_eq!(back.kind_count(), 2);
    assert_eq!(*back.kind_table(), *stock_kinds());
}

#[test]
fn shipped_levels_still_load_with_the_stock_table() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/levels");
    for file in ["showcase.msl", "corridor.msl", "facility.msl", "facing.msl"] {
        let bytes = std::fs::read(format!("{dir}/{file}")).unwrap();
        let level = format::read(&bytes).unwrap_or_else(|e| panic!("{file}: {e:?}"));
        assert!(level.kinds.is_empty(), "{file}");
    }
}

#[test]
fn kinds_round_trip_in_binary() {
    let level = three_kinds();
    let back = refused(&level).unwrap();
    assert_eq!(back, level);
    assert_eq!(back.kind_table()[2].name, "mantis");
}

#[test]
fn kinds_round_trip_in_text() {
    let level = three_kinds();
    let text = to_text(&level);
    assert!(text.contains("kind mantis hp=100 radius=0.75"), "{text}");
    assert!(text.contains("| miniscent-blueprint 1"));
    assert_eq!(from_text(&text).unwrap(), level);
}

#[test]
fn a_version_six_file_does_not_read_kinds() {
    let level = Level {
        entities: vec![enemy(1)],
        kinds: vec![kind("a", MANTIS), kind("b", MANTIS)],
        ..Level::default()
    };
    let mut bytes = format::write(&level);
    bytes[4..6].copy_from_slice(&6u16.to_le_bytes());
    // Section 6 is unknown to version 6, so it is skipped like any other.
    assert!(format::read(&bytes).unwrap().kinds.is_empty());
}

#[test]
fn every_truncation_is_an_error() {
    let bytes = format::write(&three_kinds());
    for end in 0..bytes.len() {
        assert!(format::read(&bytes[..end]).is_err(), "cut at {end}");
    }
}

#[test]
fn flipped_bytes_never_panic() {
    let bytes = format::write(&three_kinds());
    let mut rng = Rng::new(0x6b1d5);
    let len = u32::try_from(bytes.len()).unwrap();
    for _ in 0..5_000 {
        let mut data = bytes.clone();
        for _ in 0..=rng.below(4) {
            let i = rng.below(len) as usize;
            data[i] ^= 1 << rng.below(8);
        }
        if let Ok(level) = format::read(&data) {
            assert!(format::read(&format::write(&level)).is_ok());
        }
    }
}

#[test]
fn too_many_kinds_are_refused() {
    let names = (0..=MAX_LEVEL_KINDS).map(|i| format!("k{i}"));
    let level = Level {
        kinds: names.map(|n| kind(&n, MANTIS)).collect(),
        ..Level::default()
    };
    assert_eq!(refused(&level), Err(FormatError::Kinds(KindError::TooMany)));
    let mut text = to_text(&level);
    assert!(from_text(&text).unwrap_err().msg.contains("too many kinds"));
    // One fewer is fine as far as the count goes.
    let at = text.rfind("kind k32").unwrap();
    text.truncate(at);
    assert!(from_text(&text).is_ok());
}

#[test]
fn bad_names_are_refused() {
    let long = "a".repeat(MAX_KIND_NAME + 1);
    for name in ["", "Drone", "a b", "caf\u{e9}", long.as_str()] {
        let level = Level {
            kinds: vec![kind(name, MANTIS)],
            ..Level::default()
        };
        assert_eq!(
            refused(&level),
            Err(FormatError::Kinds(KindError::BadName(0))),
            "{name:?}"
        );
    }
}

#[test]
fn a_repeated_name_is_refused() {
    let level = Level {
        kinds: vec![kind("same", MANTIS), kind("same", MANTIS)],
        ..Level::default()
    };
    let err = Err(FormatError::Kinds(KindError::DuplicateName(1)));
    assert_eq!(refused(&level), err);
}

#[test]
fn every_stat_out_of_range_is_refused() {
    let tweaks: [fn(&mut KindStats); 12] = [
        |s| s.hp = 2_000_000,
        |s| s.damage = 2_000_000,
        |s| s.radius = 0.0,
        |s| s.radius = -1.0,
        |s| s.radius = 65.0,
        |s| s.radius = f32::NAN,
        |s| s.speed = -1.0,
        |s| s.sight = f32::INFINITY,
        |s| s.attack_range = 2.0e6,
        |s| s.cooldown = -0.5,
        |s| s.cooldown = 4000.0,
        |s| s.lose = f32::NAN,
    ];
    for (i, tweak) in tweaks.iter().enumerate() {
        let mut k = kind("k", MANTIS);
        tweak(&mut k.stats);
        let level = Level {
            kinds: vec![k],
            ..Level::default()
        };
        let err = Err(FormatError::Kinds(KindError::BadStats(0)));
        assert_eq!(refused(&level), err, "tweak {i}");
    }
}

#[test]
fn an_enemy_past_the_table_is_refused() {
    let mut level = three_kinds();
    level.entities.push(enemy(3));
    let err = Err(FormatError::Kinds(KindError::NoSuchKind(3)));
    assert_eq!(refused(&level), err);
    // With no kinds of its own, the stock table has two.
    let stock = Level {
        entities: vec![enemy(0), enemy(2)],
        ..Level::default()
    };
    let err = Err(FormatError::Kinds(KindError::NoSuchKind(1)));
    assert_eq!(refused(&stock), err);
}

#[test]
fn a_bad_blueprint_is_refused() {
    let mut bytes = format::write(&three_kinds());
    let header = b"miniscent-blueprint 1";
    let at = bytes
        .windows(header.len())
        .position(|w| w == header)
        .unwrap();
    bytes[at] = b'X';
    let err = Err(FormatError::Kinds(KindError::BadBlueprint(0)));
    assert_eq!(format::read(&bytes), err);
}

/// A blueprint that reads and costs at least a `share` of the level's limit,
/// yet less than one model's.
fn costly(share: u64) -> String {
    let head = "miniscent-blueprint 1\nname heavy\nclass drone\nskin hull metal\n";
    let mut text = format!("{head}part p cell=0.25\nadd sphere 0 0 0 8 hull\n");
    loop {
        let b = model::read(text.as_bytes()).unwrap();
        let ns = estimated_ns(&b);
        assert!(ns < MAX_MODEL_NS, "one model went past its own limit");
        if ns * share > MAX_LEVEL_MODEL_NS {
            return text;
        }
        text += "smooth sphere 0 0 0 12\n";
    }
}

#[test]
fn a_level_too_costly_to_build_is_refused_before_building() {
    let text = costly(MAX_LEVEL_MODEL_NS / MAX_MODEL_NS + 1);
    let n = MAX_LEVEL_MODEL_NS / MAX_MODEL_NS + 1;
    let kinds: Vec<Kind> = (0..n).map(|i| kind(&format!("h{i}"), &text)).collect();
    let level = Level {
        kinds,
        ..Level::default()
    };
    let start = std::time::Instant::now();
    assert_eq!(
        refused(&level),
        Err(FormatError::Kinds(KindError::TooCostly))
    );
    // Judged from the text alone: far less than one build.
    assert!(start.elapsed().as_millis() < 100, "{:?}", start.elapsed());
}

fn text_error(level_text: &str) -> (usize, String) {
    let e = from_text(level_text).unwrap_err();
    (e.line, e.msg)
}

#[test]
fn text_errors_name_their_line() {
    let ok = "kind a hp=1 radius=1 speed=1 sight=1 attack=1 damage=1 cooldown=1 lose=1";
    let body = "| miniscent-blueprint 1\n| name a\n| class drone\n| skin hull metal\n";
    let part = "| part p\n| add sphere 0 0 0 2 hull\nend\n";
    let head = "miniscent-level 1\n";
    let cases = [
        (
            format!("{head}kind a hp=1\n{body}{part}"),
            2,
            "radius is missing",
        ),
        (format!("{head}{ok} hp=2\n{body}{part}"), 2, "given twice"),
        (format!("{head}{ok} armour=3\n{body}{part}"), 2, "no stat"),
        (format!("{head}{ok}\n{body}"), 2, "no end"),
        (format!("{head}{ok}\n{body}box\n"), 7, "expected '|' or end"),
        (
            format!("{head}{ok}\n{body}| part p\n| add cube\nend\n"),
            8,
            "in kind 'a'",
        ),
    ];
    for (text, line, msg) in cases {
        let (got_line, got) = text_error(&text);
        assert_eq!(got_line, line, "{got}");
        assert!(got.contains(msg), "{got}");
    }
    // Stats are judged by the binary reader, for the level as a whole.
    let bad = format!("{head}{}\n{body}{part}", ok.replace("radius=1", "radius=0"));
    assert!(text_error(&bad).1.contains("BadStats"));
}
