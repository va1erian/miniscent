use mscent_math::Vec3d;
use mscent_math::rng::Rng;

use super::test_util::{coords, gyroid, mesh_all, random_point};
use super::wire::{MAX_DETAIL_CHUNKS, MAX_VOLUMES, read_for_test};
use super::{CHUNK_SAMPLES, CellSize, Chunk, Context, DabOp, Prim, Volume, apply_dab, fill};
use crate::brush::{Brush, Op};
use crate::entity::{Entity, Level};
use crate::format::{self, FormatError};
use crate::format_wire::{put_u16, put_u32};

fn room() -> Vec<Brush> {
    let (lo, hi) = (Vec3d::new(-8.0, -8.0, 0.0), Vec3d::new(8.0, 8.0, 8.0));
    vec![Brush::cuboid(lo, hi, Op::Subtract, 0).expect("valid")]
}

fn sculpted(op: Op, origin: [i32; 3], seed: u64) -> Volume {
    let brushes = room();
    let mut v = Volume::new(op, 1, CellSize::Quarter, origin, [2, 2, 1]).expect("valid");
    let mut ctx = Context::new(&v, &brushes);
    let mut rng = Rng::new(seed);
    for i in 0..20u8 {
        let p = random_point(&mut rng, &v);
        let prim = Prim::sphere(p, rng.range_f64(0.3, 1.5)).expect("valid");
        let dab = if i % 2 == 0 {
            DabOp::Add { material: i }
        } else {
            DabOp::Carve
        };
        apply_dab(&mut v, &mut ctx, &prim, dab);
    }
    v
}

fn level(volumes: Vec<Volume>) -> Level {
    Level {
        brushes: room(),
        entities: vec![Entity::PlayerStart {
            pos: mscent_math::Vec3::new(0.0, 0.0, 2.0),
            yaw: 0.0,
        }],
        volumes,
        ..Default::default()
    }
}

/// The start of a volume record, up to its chunks.
fn head(out: &mut Vec<u8>, op: u8, cell: u8, origin: [i32; 3], chunks: [u16; 3]) {
    put_u32(out, 1);
    out.extend_from_slice(&[op, cell]);
    for o in origin {
        out.extend_from_slice(&o.to_le_bytes());
    }
    for c in chunks {
        put_u16(out, c);
    }
}

/// A section with one volume of one chunk, whose bytes are `chunk`.
fn one_chunk(chunk: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    put_u32(&mut out, 1);
    head(&mut out, 0, 1, [0; 3], [1, 1, 1]);
    out.extend_from_slice(chunk);
    out
}

/// A plane of one run: all samples `value`.
fn flat_plane(out: &mut Vec<u8>, value: u8) {
    put_u32(out, 3);
    put_u16(out, CHUNK_SAMPLES as u16);
    out.push(value);
}

/// A detail chunk with two different deltas, so it stays detail.
fn minimal_detail(out: &mut Vec<u8>) {
    out.push(1);
    put_u32(out, 6);
    put_u16(out, 1);
    out.push(1);
    put_u16(out, CHUNK_SAMPLES as u16 - 1);
    out.push(2);
    flat_plane(out, 0);
}

#[test]
fn volumes_round_trip_exactly_and_mesh_the_same() {
    let a = sculpted(Op::Add, [0; 3], 1);
    let b = sculpted(Op::Subtract, [32, 0, 0], 2);
    let original = level(vec![a.clone(), b]);
    let bytes = format::write(&original);
    let back = format::read(&bytes).expect("reads");
    assert_eq!(back, original);
    assert_eq!(mesh_all(&back.volumes[0], &room()), mesh_all(&a, &room()));
    assert_eq!(format::write(&back), bytes, "writing again is stable");
}

#[test]
fn noise_is_stored_plain_and_comes_back() {
    let mut v = Volume::new(Op::Add, 1, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid");
    let mut ctx = Context::new(&v, &[]);
    let mut rng = Rng::new(3);
    let noise: Vec<f64> = (0..CHUNK_SAMPLES)
        .map(|_| rng.range_f64(-0.5, 0.5))
        .collect();
    let at = |p: Vec3d| {
        let g = (p * 4.0).to_f32();
        noise[g.x as usize + g.y as usize * 16 + g.z as usize * 256]
    };
    fill(&mut v, &mut ctx, at, 0);
    let bytes = format::write(&level(vec![v.clone()]));
    let back = format::read(&bytes).expect("reads");
    assert_eq!(back.volumes, vec![v]);
    assert!(bytes.len() < 3 * CHUNK_SAMPLES, "{} bytes", bytes.len());
}

#[test]
fn real_shapes_store_far_below_their_samples() {
    let mut v = Volume::new(Op::Subtract, 1, CellSize::Quarter, [0; 3], [4, 4, 2]).expect("valid");
    let mut ctx = Context::new(&v, &[]);
    fill(&mut v, &mut ctx, gyroid, 2);
    let details = coords(&v)
        .iter()
        .filter(|c| matches!(v.chunk(**c), Some(Chunk::Detail(_))))
        .count();
    let empty = format::write(&level(Vec::new())).len();
    let stored = format::write(&level(vec![v])).len() - empty;
    assert!(
        stored * 2 < details * 2 * CHUNK_SAMPLES,
        "{stored} of {details}"
    );
}

#[test]
fn a_level_without_volumes_has_no_section_for_them() {
    let bytes = format::write(&level(Vec::new()));
    assert_eq!(
        bytes[8..12],
        2u32.to_le_bytes(),
        "brushes and entities only"
    );
    assert_eq!(format::read(&bytes).expect("reads").volumes, Vec::new());
}

#[test]
fn older_versions_still_load_with_no_volumes() {
    for version in [2u16, 3] {
        let mut bytes = format::write(&level(Vec::new()));
        bytes[4..6].copy_from_slice(&version.to_le_bytes());
        assert_eq!(format::read(&bytes), Ok(level(Vec::new())));
        // Section 5 meant nothing to these versions: it is skipped like any unknown one.
        let mut with = format::write(&level(vec![sculpted(Op::Add, [0; 3], 1)]));
        with[4..6].copy_from_slice(&version.to_le_bytes());
        assert_eq!(format::read(&with), Ok(level(Vec::new())));
    }
}

#[test]
fn damaged_files_never_panic_and_whatever_loads_can_be_meshed() {
    let volumes = vec![
        sculpted(Op::Add, [0; 3], 1),
        sculpted(Op::Subtract, [32, 0, 0], 2),
    ];
    let good = format::write(&level(volumes));
    let mut rng = Rng::new(77);
    let (mut loaded, mut refused) = (0, 0);
    for _ in 0..400 {
        let mut bad = good.clone();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(bad.len() as u32) as usize;
            bad[at] ^= 1 << rng.below(8);
        }
        if rng.below(4) == 0 {
            bad.truncate(rng.below(bad.len() as u32) as usize);
        }
        match format::read(&bad) {
            Ok(l) => {
                loaded += 1;
                for v in &l.volumes {
                    mesh_all(v, &l.brushes);
                }
            }
            Err(_) => refused += 1,
        }
    }
    assert!(
        loaded > 0 && refused > 0,
        "{loaded} loaded, {refused} refused"
    );
}

#[test]
fn forged_runs_are_refused() {
    let detail = |plane: &[u8]| {
        let mut chunk = vec![1];
        chunk.extend_from_slice(plane);
        flat_plane(&mut chunk, 0);
        one_chunk(&chunk)
    };
    let plane = |runs: &[(u16, u8)]| {
        let mut out = Vec::new();
        put_u32(&mut out, (runs.len() * 3) as u32);
        for &(count, value) in runs {
            put_u16(&mut out, count);
            out.push(value);
        }
        out
    };
    let bad = FormatError::BadVolume;
    let full = CHUNK_SAMPLES as u16;
    let read = |b: &[u8]| read_for_test(b, 1).map(|_| ());
    assert_eq!(read(&detail(&plane(&[(full, 1)]))), Ok(()));
    assert_eq!(read(&detail(&plane(&[(full - 1, 1)]))), Err(bad), "short");
    assert_eq!(
        read(&detail(&plane(&[(full, 1), (1, 1)]))),
        Err(bad),
        "over"
    );
    assert_eq!(
        read(&detail(&plane(&[(u16::MAX, 1)]))),
        Err(bad),
        "far over"
    );
    assert_eq!(
        read(&detail(&plane(&[(0, 1), (full, 1)]))),
        Err(bad),
        "empty run"
    );
    assert_eq!(
        read(&detail(&plane(&[(full, 127)]))),
        Err(bad),
        "delta range"
    );
    assert_eq!(
        read(&detail(&plane(&[(full, 0x80)]))),
        Err(bad),
        "delta -128"
    );
    let mut odd = Vec::new();
    put_u32(&mut odd, 4);
    odd.extend_from_slice(&[1, 0, 1, 0]);
    assert_eq!(read(&detail(&odd)), Err(bad), "not whole runs");
    let mut long = Vec::new();
    put_u32(&mut long, u32::MAX);
    assert_eq!(
        read(&detail(&long)),
        Err(FormatError::Truncated),
        "past the end"
    );
}

#[test]
fn detail_chunks_are_charged_before_they_are_allocated() {
    let mut body = Vec::new();
    put_u32(&mut body, 1);
    head(&mut body, 0, 1, [0; 3], [4, 1, 1]);
    for _ in 0..4 {
        minimal_detail(&mut body);
    }
    assert!(read_for_test(&body, 4).is_ok());
    assert_eq!(read_for_test(&body, 3).err(), Some(FormatError::TooMany));
}

#[test]
fn a_small_file_cannot_ask_for_more_than_the_detail_limit() {
    // Five volumes of a full 16^3 chunks each, in far less than a chunk's
    // samples per chunk: the 16 385th detail chunk ends the read.
    let volumes = 5;
    let mut body = Vec::new();
    put_u32(&mut body, volumes);
    for i in 0..volumes as i32 {
        head(&mut body, 0, 1, [i * 256 * 2, 0, 0], [16, 16, 16]);
        for _ in 0..4096 {
            minimal_detail(&mut body);
        }
    }
    assert!(MAX_DETAIL_CHUNKS < volumes as usize * 4096);
    assert!(body.len() < MAX_DETAIL_CHUNKS * 32);
    assert_eq!(
        read_for_test(&body, MAX_DETAIL_CHUNKS).err(),
        Some(FormatError::TooMany)
    );
}

#[test]
fn counts_are_checked_against_the_bytes_there() {
    let mut body = Vec::new();
    put_u32(&mut body, MAX_VOLUMES as u32 + 1);
    body.resize(4 + (MAX_VOLUMES + 1) * 64, 0);
    assert_eq!(read_for_test(&body, 1).err(), Some(FormatError::TooMany));
    let mut huge = Vec::new();
    put_u32(&mut huge, u32::MAX);
    assert_eq!(read_for_test(&huge, 1).err(), Some(FormatError::TooMany));
    // A full table with no chunks after it is refused before it is built.
    let mut empty = Vec::new();
    put_u32(&mut empty, 1);
    head(&mut empty, 0, 1, [0; 3], [16, 16, 16]);
    assert_eq!(read_for_test(&empty, 1).err(), Some(FormatError::TooMany));
}

#[test]
fn bad_fields_are_refused() {
    let with = |op: u8, cell: u8, origin: [i32; 3], chunks: [u16; 3]| {
        let mut body = Vec::new();
        put_u32(&mut body, 1);
        head(&mut body, op, cell, origin, chunks);
        body.extend_from_slice(&[0, 0, 0]);
        read_for_test(&body, 1).map(|_| ())
    };
    let bad = Err(FormatError::BadVolume);
    assert_eq!(with(0, 1, [0; 3], [1, 1, 1]), Ok(()));
    assert_eq!(with(2, 1, [0; 3], [1, 1, 1]), Err(FormatError::BadOp(2)));
    // Cell codes 4 and 5 are the coarse cells; 6 is none.
    assert_eq!(with(0, 4, [0; 3], [1, 1, 1]), Ok(()));
    assert_eq!(with(0, 5, [0; 3], [1, 1, 1]), Ok(()));
    assert_eq!(with(0, 6, [0; 3], [1, 1, 1]), bad);
    assert_eq!(with(0, 1, [0; 3], [0, 1, 1]), bad);
    assert_eq!(with(0, 1, [0; 3], [17, 1, 1]), bad);
    assert_eq!(with(0, 1, [i32::MAX; 3], [1, 1, 1]), bad);
    assert_eq!(with(0, 1, [i32::MIN; 3], [1, 1, 1]), bad);
    let chunk_of = |bytes: &[u8]| read_for_test(&one_chunk(bytes), 1).map(|_| ());
    assert_eq!(chunk_of(&[2, 0, 0]), bad, "unknown kind");
    assert_eq!(chunk_of(&[0, 127, 0]), bad, "uniform delta range");
    assert_eq!(chunk_of(&[0, 0, 0, 9]), Err(FormatError::Trailing));
}

#[test]
fn volumes_may_touch_but_not_overlap() {
    let at = |x: i32| sculpted(Op::Add, [x, 0, 0], 1);
    let touching = format::write(&level(vec![at(0), at(32)]));
    assert!(format::read(&touching).is_ok());
    let overlapping = format::write(&level(vec![at(0), at(31)]));
    assert_eq!(format::read(&overlapping), Err(FormatError::BadVolume));
}

#[test]
fn a_detail_chunk_of_one_value_loads_as_a_uniform_one() {
    let mut chunk = vec![1];
    flat_plane(&mut chunk, 5);
    flat_plane(&mut chunk, 2);
    let volumes = read_for_test(&one_chunk(&chunk), 1).expect("reads");
    assert_eq!(
        volumes[0].chunk([0, 0, 0]),
        Some(&Chunk::Uniform {
            delta: 5,
            material: 2
        })
    );
}

#[test]
fn a_volume_sharing_an_order_with_a_brush_is_refused() {
    let level_at = |order: u32| Level {
        brushes: vec![
            Brush::cuboid(
                Vec3d::new(0.0, 0.0, 0.0),
                Vec3d::new(8.0, 8.0, 8.0),
                Op::Add,
                3,
            )
            .expect("valid"),
        ],
        volumes: vec![
            Volume::new(Op::Add, order, CellSize::Quarter, [0; 3], [1, 1, 1]).expect("valid"),
        ],
        ..Default::default()
    };
    let tied = format::write(&level_at(3));
    assert_eq!(format::read(&tied), Err(FormatError::BadVolume));
    let apart = level_at(4);
    assert_eq!(format::read(&format::write(&apart)), Ok(apart));
}

#[test]
fn a_small_file_cannot_ask_for_more_than_the_chunk_limit() {
    // Seventeen full volumes of uniform chunks, three bytes each.
    let volumes = 17;
    let mut body = Vec::new();
    put_u32(&mut body, volumes);
    for i in 0..volumes as i32 {
        head(&mut body, 0, 1, [i * 256 * 2, 0, 0], [16, 16, 16]);
        for _ in 0..4096 {
            body.extend_from_slice(&[0, 0, 0]);
        }
    }
    assert!(super::MAX_TOTAL_CHUNKS < volumes as usize * 4096);
    assert_eq!(
        read_for_test(&body, super::MAX_DETAIL_CHUNKS).err(),
        Some(FormatError::TooMany)
    );
}
