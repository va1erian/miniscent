//! TexInfo plumbing: brushes, face extraction and the level file.

use crate::brush::{Brush, BrushError, Op};
use crate::entity::Level;
use crate::format::{self, FormatError};
use crate::texinfo::TexInfo;
use crate::{compile, extract_faces};
use mscent_math::{DPlane, Vec3d};

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn cube(lo: f64, hi: f64, op: Op, order: u32) -> Brush {
    Brush::cuboid(v(lo, lo, lo), v(hi, hi, hi), op, order).expect("cube")
}

fn rotated(mut t: TexInfo, deg: f32) -> TexInfo {
    t.rotate(deg);
    t
}

#[test]
fn brush_defaults_follow_each_plane() {
    let b = cube(0.0, 4.0, Op::Add, 0);
    assert_eq!(b.texinfos().len(), b.planes().len());
    for (t, p) in b.texinfos().iter().zip(b.planes()) {
        assert_eq!(*t, TexInfo::default_for_normal(p.normal));
    }
    let m = b.with_material(3);
    assert!(m.texinfos().iter().all(|t| t.material == 3));
    assert_eq!(m.material(), 3);
}

#[test]
fn texinfos_follow_their_planes_when_redundant_planes_drop() {
    let b = cube(0.0, 4.0, Op::Add, 0);
    // A far-away redundant plane first, then the real ones with marks.
    let mut planes = vec![DPlane::new(v(1.0, 0.0, 0.0), 100.0)];
    planes.extend_from_slice(b.planes());
    let mark = |i: usize| TexInfo {
        material: i as u16,
        ..TexInfo::default_for_normal(planes[i].normal)
    };
    let infos: Vec<TexInfo> = (0..planes.len()).map(mark).collect();
    let rebuilt = Brush::new_textured(&planes, &infos, Op::Add, 0).expect("valid");
    assert_eq!(rebuilt.planes().len(), 6);
    for (t, p) in rebuilt.texinfos().iter().zip(rebuilt.planes()) {
        let src = planes.iter().position(|q| q == p).expect("plane kept");
        assert_eq!(usize::from(t.material), src);
    }
    assert_eq!(
        Brush::new_textured(&planes, &infos[1..], Op::Add, 0),
        Err(BrushError::Degenerate)
    );
}

#[test]
fn set_texinfo_validates_the_index_and_repairs_values() {
    let mut b = cube(0.0, 4.0, Op::Add, 0);
    let bad = TexInfo {
        u_scale: f32::NAN,
        ..b.texinfos()[0]
    };
    assert!(!b.set_texinfo(99, bad));
    assert!(b.set_texinfo(0, bad));
    assert!(b.texinfos()[0].is_valid());
    let top = b.plane_facing(v(0.0, 0.0, 1.0)).expect("a plane");
    assert_eq!(b.planes()[top].normal, v(0.0, 0.0, 1.0));
    assert_eq!(b.plane_facing(Vec3d::ZERO), None);
}

fn floor_faces(level: &Level) -> Vec<crate::Face> {
    let bsp = compile(&level.brushes).expect("compiles");
    extract_faces(&bsp, &level.brushes)
        .into_iter()
        .filter(|f| f.plane.normal == v(0.0, 0.0, 1.0) && f.plane.dist.abs() < 1e-9)
        .collect()
}

fn level_of(brushes: Vec<Brush>) -> Level {
    Level {
        brushes,
        entities: Vec::new(),
    }
}

#[test]
fn faces_keep_the_texinfo_after_clipping_and_never_merge_across_texinfos() {
    let mut room = Brush::cuboid(v(0.0, 0.0, 0.0), v(10.0, 10.0, 6.0), Op::Subtract, 0).unwrap();
    let floor = room.plane_facing(v(0.0, 0.0, -1.0)).expect("floor plane");
    let marked = rotated(room.texinfos()[floor], 30.0);
    assert!(room.set_texinfo(floor, marked));
    // A pillar clips the floor into several pieces.
    let pillar = Brush::cuboid(v(4.0, 4.0, 0.0), v(6.0, 6.0, 6.0), Op::Add, 1).unwrap();
    let faces = floor_faces(&level_of(vec![room.clone(), pillar]));
    assert!(faces.len() >= 4, "the pillar splits the floor");
    assert!(faces.iter().all(|f| f.texinfo == marked));
    let area: f64 = faces.iter().map(|f| f.polygon.area()).sum();
    assert!((area - 96.0).abs() < 1e-6, "{area}");

    // A second room continues the same floor plane with another mapping:
    // the two floors join without a wall but must never merge.
    let mut annex = Brush::cuboid(v(10.0, 0.0, 0.0), v(20.0, 10.0, 6.0), Op::Subtract, 1).unwrap();
    let floor = annex.plane_facing(v(0.0, 0.0, -1.0)).expect("floor plane");
    let other = TexInfo {
        material: 4,
        ..rotated(annex.texinfos()[floor], -45.0)
    };
    assert!(annex.set_texinfo(floor, other));
    let faces = floor_faces(&level_of(vec![room, annex]));
    let area_of = |t: &TexInfo| -> f64 {
        faces
            .iter()
            .filter(|f| f.texinfo == *t)
            .map(|f| f.polygon.area())
            .sum()
    };
    assert!((area_of(&marked) - 100.0).abs() < 1e-6);
    assert!((area_of(&other) - 100.0).abs() < 1e-6);
    assert!(faces.iter().all(|f| f.material == f.texinfo.material));
}

fn marked_level() -> Level {
    let mut b = cube(0.0, 8.0, Op::Subtract, 0);
    for i in 0..b.planes().len() {
        let mut t = b.texinfos()[i];
        t.material = i as u16;
        t.rotate(10.0 * i as f32);
        t.set_scale(0.0625 * (i + 1) as f32, -0.25);
        t.shift_by(3.5 * i as f32, -2.0);
        t.lock_to_world = i % 2 == 0;
        assert!(b.set_texinfo(i, t));
    }
    level_of(vec![b, cube(1.0, 2.0, Op::Add, 1).with_material(5)])
}

#[test]
fn texinfo_round_trips_through_the_level_file() {
    let level = marked_level();
    let bytes = format::write(&level);
    assert_eq!(&bytes[4..6], &2u16.to_le_bytes());
    let back = format::read(&bytes).expect("reads");
    assert_eq!(back, level);
    assert_eq!(format::write(&back), bytes);
}

/// Byte offset of the first brush's first TexInfo record in `write` output.
fn first_texinfo_offset(level: &Level) -> usize {
    // header 12, section id+len 8, count 4, op 1, order 4, material 2, np 4.
    12 + 8 + 4 + 1 + 4 + 2 + 4 + level.brushes[0].planes().len() * 32
}

#[test]
fn version_one_files_load_with_default_mappings() {
    let level = level_of(vec![cube(0.0, 8.0, Op::Subtract, 0).with_material(2)]);
    let mut brushes = Vec::new();
    brushes.extend_from_slice(&1u32.to_le_bytes());
    brushes.push(1);
    brushes.extend_from_slice(&0u32.to_le_bytes());
    brushes.extend_from_slice(&2u16.to_le_bytes());
    let planes = level.brushes[0].planes();
    brushes.extend_from_slice(&(planes.len() as u32).to_le_bytes());
    for p in planes {
        for x in [p.normal.x, p.normal.y, p.normal.z, p.dist] {
            brushes.extend_from_slice(&x.to_le_bytes());
        }
    }
    let mut file = b"MSLV".to_vec();
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&0u16.to_le_bytes());
    file.extend_from_slice(&2u32.to_le_bytes());
    for (id, body) in [(1u32, brushes), (2, 0u32.to_le_bytes().to_vec())] {
        file.extend_from_slice(&id.to_le_bytes());
        file.extend_from_slice(&(body.len() as u32).to_le_bytes());
        file.extend_from_slice(&body);
    }
    let read = format::read(&file).expect("v1 still loads");
    assert_eq!(read, level);
    assert!(read.brushes[0].texinfos().iter().all(|t| t.material == 2));
    let mut future = file.clone();
    future[4..6].copy_from_slice(&3u16.to_le_bytes());
    assert_eq!(
        format::read(&future),
        Err(FormatError::UnsupportedVersion(3))
    );
}

#[test]
fn hostile_texinfo_fields_are_rejected() {
    let level = marked_level();
    let bytes = format::write(&level);
    let at = first_texinfo_offset(&level);
    // Record: material 2, u_axis 12, v_axis 12, shifts 8, scales 8, rot 4, lock 1.
    let corrupt = |offset: usize, patch: &[u8]| {
        let mut b = bytes.clone();
        b[at + offset..at + offset + patch.len()].copy_from_slice(patch);
        format::read(&b)
    };
    let bad = Err(FormatError::BadTexInfo);
    assert_eq!(corrupt(2, &f32::NAN.to_le_bytes()), bad); // u_axis.x
    assert_eq!(corrupt(2, &[0; 12]), bad); // zero axis
    assert_eq!(corrupt(26, &f32::NAN.to_le_bytes()), bad); // u_shift
    assert_eq!(corrupt(34, &0f32.to_le_bytes()), bad); // u_scale 0
    assert_eq!(corrupt(38, &1e30f32.to_le_bytes()), bad); // v_scale huge
    assert_eq!(corrupt(42, &f32::INFINITY.to_le_bytes()), bad); // rotation
    assert_eq!(corrupt(46, &[2]), bad); // lock flag
    assert!(corrupt(46, &[1]).is_ok());
}

#[test]
fn truncated_and_flipped_files_never_panic() {
    let bytes = format::write(&marked_level());
    for n in 0..bytes.len() {
        assert!(format::read(&bytes[..n]).is_err(), "truncated at {n}");
    }
    let mut rng = mscent_math::rng::Rng::new(0x7e57);
    for _ in 0..400 {
        let mut b = bytes.clone();
        for _ in 0..3 {
            let i = rng.next_u64() as usize % b.len();
            b[i] = rng.next_u64() as u8;
        }
        let _ = format::read(&b);
    }
}
