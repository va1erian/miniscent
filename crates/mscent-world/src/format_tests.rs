use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::format::{self, FormatError, MAX_BRUSHES};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};

fn sample() -> Level {
    let v = Vec3d::new;
    let p = Vec3::new;
    Level {
        brushes: vec![
            Brush::cuboid(v(-8.0, -8.0, -8.0), v(8.0, 8.0, 8.0), Op::Subtract, 0).unwrap(),
            Brush::cuboid(v(-1.0, -1.0, -8.0), v(1.0, 1.0, 0.0), Op::Add, 1)
                .unwrap()
                .with_material(7),
        ],
        entities: vec![
            Entity::PlayerStart {
                pos: p(0.0, 0.0, 2.0),
                yaw: 1.5,
            },
            Entity::Light {
                pos: p(0.0, 4.0, 4.0),
                color: p(1.0, 0.5, 0.25),
                radius: 12.0,
                intensity: 2.0,
            },
            Entity::Enemy {
                pos: p(5.0, 5.0, 1.0),
                kind: 3,
            },
            Entity::Goal {
                pos: p(-6.0, 6.0, 1.0),
                radius: 1.5,
            },
        ],
        volumes: Vec::new(),
    }
}

fn put(bytes: &mut [u8], at: usize, v: u32) {
    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

#[test]
fn round_trips() {
    let level = sample();
    let bytes = format::write(&level);
    assert_eq!(format::read(&bytes).unwrap(), level);
    assert_eq!(format::write(&format::read(&bytes).unwrap()), bytes);
}

#[test]
fn empty_level_round_trips() {
    let level = Level::default();
    assert_eq!(format::read(&format::write(&level)).unwrap(), level);
}

#[test]
fn every_truncation_is_an_error() {
    let bytes = format::write(&sample());
    for cut in 0..bytes.len() {
        assert!(format::read(&bytes[..cut]).is_err(), "cut at {cut}");
    }
}

#[test]
fn header_errors() {
    let good = format::write(&sample());
    let mut bad = good.clone();
    bad[0] = b'X';
    assert_eq!(format::read(&bad), Err(FormatError::BadMagic));
    let mut bad = good.clone();
    bad[4] = 9;
    assert_eq!(format::read(&bad), Err(FormatError::UnsupportedVersion(9)));
    let mut bad = good.clone();
    bad[6] = 1;
    assert_eq!(format::read(&bad), Err(FormatError::BadFlags));
    let mut bad = good;
    bad.push(0);
    assert_eq!(format::read(&bad), Err(FormatError::Trailing));
}

#[test]
fn huge_counts_do_not_allocate_or_panic() {
    let good = format::write(&sample());
    // Header is 12 bytes, then a section head (id, len) of 8, then the count.
    let mut bad = good.clone();
    put(&mut bad, 20, u32::MAX);
    assert_eq!(format::read(&bad), Err(FormatError::TooMany));
    let mut bad = good.clone();
    put(&mut bad, 20, u32::try_from(MAX_BRUSHES + 1).unwrap());
    assert_eq!(format::read(&bad), Err(FormatError::TooMany));
    let mut bad = good.clone();
    put(&mut bad, 8, u32::MAX);
    assert_eq!(format::read(&bad), Err(FormatError::TooMany));
    let mut bad = good;
    put(&mut bad, 16, u32::MAX);
    assert_eq!(format::read(&bad), Err(FormatError::Truncated));
}

#[test]
fn rejects_bad_entities() {
    let mut level = sample();
    level.entities = vec![Entity::Goal {
        pos: Vec3::new(f32::NAN, 0.0, 0.0),
        radius: 1.0,
    }];
    assert_eq!(
        format::read(&format::write(&level)),
        Err(FormatError::BadEntity)
    );
    level.entities = vec![Entity::Goal {
        pos: Vec3::new(0.0, 0.0, 0.0),
        radius: -1.0,
    }];
    assert_eq!(
        format::read(&format::write(&level)),
        Err(FormatError::BadEntity)
    );
}

#[test]
fn missing_and_duplicate_sections() {
    let good = format::write(&sample());
    let brush_len = u32::from_le_bytes(good[16..20].try_into().unwrap());
    let ent_head = 20 + usize::try_from(brush_len).unwrap();
    let mut bad = good;
    put(&mut bad, ent_head, 1);
    assert_eq!(format::read(&bad), Err(FormatError::DuplicateSection(1)));
    // An unknown id is skipped, so the entities section goes missing.
    put(&mut bad, ent_head, 77);
    assert_eq!(format::read(&bad), Err(FormatError::MissingSection(2)));
}

/// Mutates `bytes` at random, then checks `read` returns without panicking
/// and that anything it accepts can be written and read again.
fn fuzz_once(rng: &mut Rng, bytes: &[u8]) {
    let mut data = bytes.to_vec();
    let len = u32::try_from(data.len()).unwrap();
    for _ in 0..=rng.below(4) {
        let i = rng.below(len) as usize;
        match rng.below(3) {
            0 => data[i] ^= 1 << rng.below(8),
            1 => data[i] = u8::try_from(rng.below(256)).unwrap(),
            _ => data[i] = 0xff,
        }
    }
    if let Ok(level) = format::read(&data) {
        // Brush::new renormalises planes, so bits may drift on re-read (e.g.
        // denormal components); what matters is that it is still accepted.
        assert!(format::read(&format::write(&level)).is_ok());
    }
}

#[test]
fn bit_flip_fuzzing_never_panics() {
    let bytes = format::write(&sample());
    let mut rng = Rng::new(0x5eed_1e7e1);
    for _ in 0..20_000 {
        fuzz_once(&mut rng, &bytes);
    }
}

#[test]
fn random_garbage_never_panics() {
    let mut rng = Rng::new(42);
    for _ in 0..2_000 {
        let len = rng.below(200) as usize;
        let mut data: Vec<u8> = (0..len)
            .map(|_| u8::try_from(rng.below(256)).unwrap())
            .collect();
        if rng.below(2) == 0 && data.len() >= 8 {
            data[..4].copy_from_slice(b"MSLV");
            data[4..6].copy_from_slice(&1u16.to_le_bytes());
        }
        let _ = format::read(&data);
    }
}

fn world(fog_start: f32, fog_end: f32) -> Entity {
    Entity::World {
        pos: Vec3::new(0.0, 0.0, 1.0),
        fog_start,
        fog_end,
    }
}

#[test]
fn world_entity_round_trips_and_sets_the_level_fog() {
    let mut level = sample();
    assert_eq!(level.fog(), None);
    level.entities.push(world(10.0, 80.0));
    level.entities.push(world(1.0, 2.0));
    let back = format::read(&format::write(&level)).unwrap();
    assert_eq!(back, level);
    assert_eq!(back.fog(), Some((10.0, 80.0)), "the first one counts");
}

#[test]
fn hostile_world_entities_are_rejected() {
    for (start, end) in [
        (30.0f32, 20.0),
        (-1.0, 5.0),
        (f32::NAN, 5.0),
        (1.0, f32::INFINITY),
        (1.0, 1.0e9),
        (5.0, 5.0),
    ] {
        let mut level = sample();
        level.entities.push(world(start, end));
        let r = format::read(&format::write(&level));
        assert_eq!(r, Err(FormatError::BadEntity), "{start} {end}");
    }
}
