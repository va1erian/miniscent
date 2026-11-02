use super::*;
use crate::voxel::test_util::{mesh_all, open_edges};

fn range() -> Range {
    Range {
        min: Vec3d::new(0.0, 0.0, 0.0),
        max: Vec3d::new(128.0, 128.0, 64.0),
        cell: CellSize::One,
        peak: 40.0,
        seed: 7,
        material: 4,
        order: 5,
    }
}

fn bytes(v: &Volume) -> Vec<u8> {
    super::super::wire::write_volumes(std::slice::from_ref(v))
}

#[test]
fn a_column_off_the_table_is_air_and_never_another_row() {
    let volume = mountains(&range()).unwrap();
    let heights = Heights::new(&volume, &range());
    assert!(heights.at(0, 0).is_some());
    assert_eq!(heights.at(heights.stride, 0), None);
    assert_eq!(heights.at(usize::MAX, 1), None);
    assert_eq!(heights.at(0, usize::MAX), None);
}

#[test]
fn the_same_seed_gives_the_same_bytes_and_another_seed_another_range() {
    let a = mountains(&range()).unwrap();
    assert_eq!(bytes(&a), bytes(&mountains(&range()).unwrap()));
    let other = Range { seed: 8, ..range() };
    assert_ne!(bytes(&a), bytes(&mountains(&other).unwrap()));
}

#[test]
fn the_surface_is_closed_peaks_reach_the_peak_and_nothing_stands_above_it() {
    let r = range();
    let v = mountains(&r).unwrap();
    assert_eq!(v.op(), Op::Add);
    let meshes = mesh_all(&v, &[]);
    assert!(open_edges(&meshes).is_empty(), "a hole in the surface");
    let top = meshes
        .iter()
        .flat_map(|(_, m)| m.vertices.iter().map(|v| v.pos.z))
        .fold(f32::MIN, f32::max);
    let limit = (2.0 + r.peak) as f32;
    assert!(top <= limit + 1.0, "{top} above {limit}");
    assert!(top >= limit - 4.0, "the tallest crest is the peak: {top}");
}

#[test]
fn the_range_falls_to_the_ground_at_the_edges() {
    let r = range();
    let v = mountains(&r).unwrap();
    let h = Heights::new(&v, &r);
    let ground = 2.0;
    // Every column on the border is at the ground; the middle is not.
    let n = 128usize;
    for k in 1..=n {
        for (i, j) in [(k, 1), (k, n), (1, k), (n, k)] {
            let at = h.at(i, j).unwrap();
            assert!(at <= ground + 0.5, "edge column {i},{j} at {at}");
        }
    }
    let middle = (h.z.iter().copied()).fold(f64::MIN, f64::max);
    assert!(middle > ground + 30.0);
    // Solid below the surface, air above, at a crest.
    let (i, j) = (0..h.z.len())
        .map(|k| (k % h.stride, k / h.stride))
        .max_by(|a, b| h.at(a.0, a.1).unwrap().total_cmp(&h.at(b.0, b.1).unwrap()))
        .unwrap();
    let (x, y) = ((i as f64 - 1.0) * h.cell, (j as f64 - 1.0) * h.cell);
    let crest = h.at(i, j).unwrap();
    assert!(h.field(Vec3d::new(x, y, crest - 3.0)) > 0.0);
    assert!(h.field(Vec3d::new(x, y, crest + 3.0)) < 0.0);
}

#[test]
fn hostile_ranges_are_errors_not_panics() {
    let bad = |f: fn(&mut Range)| {
        let mut r = range();
        f(&mut r);
        mountains(&r)
    };
    assert_eq!(bad(|r| r.peak = f64::NAN), Err(MountainError::NotFinite));
    assert_eq!(bad(|r| r.peak = 0.0), Err(MountainError::NotFinite));
    assert_eq!(
        bad(|r| r.max.x = f64::INFINITY),
        Err(MountainError::NotFinite)
    );
    assert_eq!(bad(|r| r.max.x = 0.0), Err(MountainError::BadBounds));
    assert_eq!(bad(|r| r.max.x = 1.0e9), Err(MountainError::BadBounds));
    assert_eq!(bad(|r| r.max.x = 4000.0), Err(MountainError::TooLarge));
    assert_eq!(bad(|r| r.peak = 100.0), Err(MountainError::PeakAboveBox));
    assert!(matches!(
        bad(|r| r.min = Vec3d::new(32000.0, 0.0, 0.0)),
        Err(MountainError::BadBounds | MountainError::Volume(_))
    ));
}

/// Shares of a 256-unit range's columns above `t` of the peak.
fn share_above(seed: u32, t: f64) -> f64 {
    let r = Range {
        min: Vec3d::new(0.0, 0.0, -3.0),
        max: Vec3d::new(256.0, 256.0, 205.0),
        cell: CellSize::Four,
        peak: 200.0,
        seed,
        ..range()
    };
    let h = Heights::new(&mountains(&r).unwrap(), &r);
    let above = h.z.iter().filter(|&&z| (z + 3.0) / 200.0 > t).count();
    above as f64 / h.z.len() as f64
}

#[test]
fn a_range_is_massifs_with_foothills_and_a_few_peaks_not_a_field_of_spikes() {
    for seed in [1, 2, 3, 13] {
        // Few peaks near the top, a broad body above a third of it, and lower
        // ground between the two and the lowlands: a third of the range at least.
        assert!(
            share_above(seed, 0.85) < 0.06,
            "seed {seed}: too many peaks"
        );
        let body = share_above(seed, 0.4);
        assert!((0.1..0.5).contains(&body), "seed {seed}: body {body}");
        let foothills = share_above(seed, 0.1) - body;
        assert!(foothills > 0.2, "seed {seed}: foothills {foothills}");
    }
}

#[test]
fn coarse_cells_still_close_the_surface_and_reach_the_peak() {
    for cell in [CellSize::Two, CellSize::Four] {
        let r = Range {
            cell,
            max: Vec3d::new(256.0, 256.0, 215.0),
            peak: 200.0,
            ..range()
        };
        let v = mountains(&r).unwrap();
        let meshes = mesh_all(&v, &[]);
        assert!(
            open_edges(&meshes).is_empty(),
            "{cell:?}: a hole in the surface"
        );
        let top = (meshes.iter())
            .flat_map(|(_, m)| m.vertices.iter().map(|v| v.pos.z))
            .fold(f32::MIN, f32::max);
        assert!(top >= 200.0 - cell.units() as f32 * 2.0, "{cell:?}: {top}");
    }
}
