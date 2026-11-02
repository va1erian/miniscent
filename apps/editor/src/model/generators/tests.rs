//! Generators are pure and bounded, whatever seed and site they are given.

use mscent_math::Vec3d;

use super::{FieldKind, Kind, MAX_PIECES, Plan, Site, field, generate, surface_normal};
use crate::model::pick::Ray;

fn v(x: f64, y: f64, z: f64) -> Vec3d {
    Vec3d::new(x, y, z)
}

fn site(size: f64) -> Site {
    Site {
        origin: v(1.0, -2.0, 3.0),
        normal: v(0.0, 0.0, 1.0),
        size,
        cell: 0.25,
        material: 4,
    }
}

fn plans(site: &Site, seed: u32) -> Vec<Plan> {
    Kind::ALL.iter().map(|&k| generate(k, site, seed)).collect()
}

#[test]
fn the_same_seed_gives_the_same_pieces_and_another_seed_gives_others() {
    let s = site(1.0);
    for kind in Kind::ALL {
        let a = generate(kind, &s, 7);
        assert!(!a.pieces().is_empty(), "{} made nothing", kind.name());
        assert_eq!(
            a.pieces(),
            generate(kind, &s, 7).pieces(),
            "{}",
            kind.name()
        );
        assert_ne!(
            a.pieces(),
            generate(kind, &s, 8).pieces(),
            "{}",
            kind.name()
        );
    }
}

#[test]
fn a_plan_is_bounded_and_grows_from_its_site_along_the_normal() {
    for size in [0.25, 1.0, 3.0, 8.0] {
        let s = site(size);
        for (kind, plan) in Kind::ALL.iter().zip(plans(&s, 3)) {
            assert!(plan.pieces().len() <= MAX_PIECES, "{}", kind.name());
            let (lo, hi) = plan.bounds().expect("not empty");
            let reach = 12.0 * size.max(0.25) + 4.0;
            assert!(
                (hi - lo).length() < 2.0 * reach,
                "{} at size {size} spans {}",
                kind.name(),
                (hi - lo).length()
            );
            assert!(hi.z > s.origin.z, "{} stays under the floor", kind.name());
            assert!(lo.z > s.origin.z - reach, "{}", kind.name());
        }
    }
}

#[test]
fn nothing_is_thinner_than_a_cell() {
    let s = site(0.25);
    let cell = s.cell;
    for (kind, plan) in Kind::ALL.iter().zip(plans(&s, 9)) {
        for piece in plan.pieces() {
            let (lo, hi) = piece.prim.bounds();
            let thin = (hi - lo).x.min((hi - lo).y).min((hi - lo).z);
            assert!(
                (thin / cell).round() >= 2.0,
                "{}: a piece {thin} across",
                kind.name()
            );
        }
    }
}

#[test]
fn a_site_that_cannot_be_built_on_gives_an_empty_plan() {
    let ok = site(1.0);
    let bad = [
        Site {
            normal: Vec3d::ZERO,
            ..ok
        },
        Site {
            normal: v(f64::NAN, 0.0, 1.0),
            ..ok
        },
        Site {
            origin: v(f64::INFINITY, 0.0, 0.0),
            ..ok
        },
        Site { size: 0.0, ..ok },
        Site {
            size: f64::NAN,
            ..ok
        },
        Site { size: 1.0e9, ..ok },
        Site { cell: 0.0, ..ok },
    ];
    for s in bad {
        for plan in plans(&s, 1) {
            assert!(plan.pieces().is_empty(), "{s:?}");
        }
    }
}

#[test]
fn growth_follows_whatever_way_the_surface_faces() {
    // Hung from a ceiling, the same drips grow downward.
    let ceiling = Site {
        normal: v(0.0, 0.0, -1.0),
        ..site(1.0)
    };
    let (lo, hi) = generate(Kind::Stalactites, &ceiling, 5)
        .bounds()
        .expect("not empty");
    assert!(lo.z < ceiling.origin.z - 1.0 && hi.z < ceiling.origin.z + 1.0);
    let wall = Site {
        normal: v(0.0, -1.0, 0.0),
        ..site(1.0)
    };
    let (lo, _) = generate(Kind::Stalactites, &wall, 5)
        .bounds()
        .expect("not empty");
    assert!(lo.y < wall.origin.y - 1.0);
}

#[test]
fn a_tree_is_thickest_at_the_foot_and_thin_where_it_ends() {
    let plan = generate(Kind::Tree, &site(1.5), 4);
    let across = |i: usize| {
        let (lo, hi) = plan.pieces()[i].prim.bounds();
        (hi - lo).x.min((hi - lo).y)
    };
    let last = plan.pieces().len() - 1;
    assert!(
        plan.pieces().len() > 100,
        "a crown of branches: {}",
        plan.pieces().len()
    );
    // The first piece is the lowest trunk section; the branches follow, and
    // the swelling and six roots of the foot come last.
    let tips = (1..last - 6).map(across).fold(f64::MAX, f64::min);
    assert!(across(0) > 3.0 * tips, "{} against {tips}", across(0));
}

#[test]
fn the_normal_of_a_slanted_plane_is_found_from_three_rays() {
    // The plane z = x / 2, hit from above.
    let hit = |r: &Ray| Some(r.origin.z - r.origin.x / 2.0);
    let ray = Ray {
        origin: v(0.0, 0.0, 5.0),
        dir: v(0.0, 0.0, -1.0),
    };
    let n = surface_normal(&ray, 0.25, hit);
    let want = v(-0.5, 0.0, 1.0).normalized().expect("unit");
    assert!((n - want).length() < 1.0e-9, "{n:?}");
    // A miss falls back to facing the viewer.
    let n = surface_normal(&ray, 0.25, |_| None);
    assert_eq!(n, v(0.0, 0.0, 1.0));
    // A zero ray still answers.
    let zero = Ray {
        origin: v(0.0, 0.0, 0.0),
        dir: Vec3d::ZERO,
    };
    assert!(surface_normal(&zero, 0.25, |_| None).is_finite());
}

#[test]
fn fields_repeat_follow_the_seed_and_survive_hostile_wavelengths() {
    let probe = |kind, seed, wave| -> Vec<f64> {
        let f = field(kind, seed, wave);
        (0..40)
            .map(|i| f(v(f64::from(i) * 0.7, f64::from(i) * 0.3, 1.0)))
            .collect()
    };
    for kind in FieldKind::ALL {
        let a = probe(kind, 3, 12.0);
        assert_eq!(a, probe(kind, 3, 12.0));
        assert_ne!(a, probe(kind, 4, 12.0), "{kind:?}");
        assert!(
            a.iter().any(|&x| x > 0.0) && a.iter().any(|&x| x < 0.0),
            "{kind:?}"
        );
        for wave in [f64::NAN, f64::INFINITY, 0.0, -5.0, 1.0e300] {
            assert!(probe(kind, 3, wave).iter().all(|x| x.is_finite()), "{wave}");
        }
    }
}
