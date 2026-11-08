use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::entity::{MAX_VISTA_FOG, MAX_VISTA_LAYERS};
use crate::format::{self, FormatError, MAX_BRUSHES};
use crate::materials::VIEW_SLOTS;
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
                kind: 1,
            },
            Entity::Goal {
                pos: p(-6.0, 6.0, 1.0),
                radius: 1.5,
            },
        ],
        ..Default::default()
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

fn vista(slot: u8, layer: u8, parallax: f32) -> Entity {
    Entity::VistaLayer {
        pos: Vec3::new(1.0, 2.0, 3.0),
        centre: Vec3::new(-4.0, 5.0, 6.0),
        slot,
        layer,
        parallax,
        spin: Vec3::new(0.0, 0.0, 0.5),
        pan: [1.5, -2.0],
        fog_start: 0.0,
        fog_end: MAX_VISTA_FOG,
        fog_color: 0x8090a0,
    }
}

fn with(e: Entity) -> Level {
    let mut level = sample();
    level.entities.push(e);
    level
}

#[test]
fn vista_layers_round_trip() {
    let mut level = with(vista(0, 0, 0.0625));
    level.entities.push(vista(7, MAX_VISTA_LAYERS - 1, 1.0));
    let bytes = format::write(&level);
    assert_eq!(format::read(&bytes).unwrap(), level);
    assert_eq!(&bytes[4..6], &format::VERSION.to_le_bytes());
}

/// A good layer with one field changed by `f`.
fn tweaked(f: impl FnOnce(&mut Entity)) -> Entity {
    let mut e = vista(1, 1, 0.5);
    f(&mut e);
    e
}

#[test]
fn hostile_vista_layers_are_rejected() {
    let cases = [
        vista(VIEW_SLOTS, 0, 0.5),
        vista(0, MAX_VISTA_LAYERS, 0.5),
        vista(0, 0, f32::NAN),
        vista(0, 0, 1.5),
        vista(0, 0, -0.1),
        tweaked(|e| {
            if let Entity::VistaLayer {
                fog_start, fog_end, ..
            } = e
            {
                (*fog_start, *fog_end) = (5.0, 5.0);
            }
        }),
        tweaked(|e| {
            if let Entity::VistaLayer { fog_end, .. } = e {
                *fog_end = MAX_VISTA_FOG * 2.0;
            }
        }),
        tweaked(|e| {
            if let Entity::VistaLayer { spin, .. } = e {
                spin.x = f32::INFINITY;
            }
        }),
        tweaked(|e| {
            if let Entity::VistaLayer { pan, .. } = e {
                pan[1] = 1.0e9;
            }
        }),
        tweaked(|e| {
            if let Entity::VistaLayer { centre, .. } = e {
                centre.y = f32::NAN;
            }
        }),
    ];
    for (i, e) in cases.into_iter().enumerate() {
        let r = format::read(&format::write(&with(e)));
        assert_eq!(r, Err(FormatError::BadEntity), "case {i}");
    }
}

/// Rewrites a file's version field.
fn as_version(mut bytes: Vec<u8>, version: u16) -> Vec<u8> {
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    bytes
}

#[test]
fn tag_seven_is_refused_before_version_five() {
    let bytes = format::write(&with(vista(0, 0, 0.5)));
    for version in 3..=4 {
        let r = format::read(&as_version(bytes.clone(), version));
        assert_eq!(r, Err(FormatError::BadEntityTag(7)), "version {version}");
    }
}

#[test]
fn a_version_four_file_without_vista_layers_still_loads() {
    // Older writers made these same bytes under a smaller number; versions 1 and
    // 2 are covered by the texinfo and labels tests.
    let level = sample();
    let bytes = format::write(&level);
    assert_eq!(format::read(&as_version(bytes, 4)).unwrap(), level);
}

#[test]
fn the_first_vista_layer_of_a_slot_and_layer_counts() {
    let mut level = with(vista(2, 1, 0.25));
    level.entities.push(vista(2, 1, 0.5));
    level.entities.push(vista(2, 2, 0.75));
    let back = format::read(&format::write(&level)).unwrap();
    let layers = back.vista_layers();
    assert_eq!(layers.len(), 2);
    assert_eq!(*layers[0], vista(2, 1, 0.25));
    assert_eq!(*layers[1], vista(2, 2, 0.75));
}

#[test]
fn bit_flips_in_a_level_with_vista_layers_never_panic() {
    let bytes = format::write(&with(vista(3, 1, 0.0625)));
    let mut rng = Rng::new(0x0051_57a5);
    for _ in 0..20_000 {
        fuzz_once(&mut rng, &bytes);
    }
}
