//! M4.4: property-based fuzzer for sphere sweeps and slide moves over
//! generated levels. Size is set by `MSCENT_FUZZ_ITERS` (moves per kind).
//!
//! Properties, judged by an oracle that evaluates the brush list directly:
//! * a sphere that starts clear of solid never ends inside it;
//! * a sweep never passes through a wall: the path up to the reported end is
//!   clear, and a sweep reported as unobstructed reaches its target;
//! * a slide never moves farther than its velocity allows, and a slide that
//!   touched nothing followed a clear straight path.

#[path = "fuzz/oracle.rs"]
mod oracle;

use mscent_collide::{MAX_BUMPS, slide_move, sweep_sphere};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_world::{Brush, Bsp, GenParams, compile, generate, room_centers};
use oracle::Oracle;

const DEFAULT_ITERS: usize = 6000;
const LEVELS: usize = 12;
const RADII: [f32; 6] = [0.05, 0.25, 0.5, 0.75, 1.0, 1.5];
/// Spacing of the reference sub-stepped path scan.
const PATH_STEP: f64 = 0.1;

struct Level {
    brushes: Vec<Brush>,
    bsp: Bsp,
    centers: Vec<Vec3d>,
}

fn iters() -> usize {
    std::env::var("MSCENT_FUZZ_ITERS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_ITERS)
}

fn make_level(seed: u64) -> Level {
    let params = GenParams {
        cols: 1 + (seed % 3) as u32,
        rows: 1 + ((seed / 3) % 3) as u32,
        extra_links: (seed % 4) as u32,
        pillar_percent: 40 + (seed % 5) as u32 * 15,
    };
    let brushes = generate(seed, &params);
    let bsp = compile(&brushes).expect("generated level compiles");
    Level {
        brushes,
        bsp,
        centers: room_centers(seed, &params),
    }
}

fn random_dir(rng: &mut Rng) -> Vec3d {
    loop {
        let v = Vec3d::new(
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
            rng.range_f64(-1.0, 1.0),
        );
        // Some directions are snapped to a plane or an axis so grazing and
        // head-on walls are common, not measure-zero.
        let v = match rng.below(4) {
            0 => Vec3d::new(v.x, v.y, 0.0),
            1 => Vec3d::new(v.x.signum(), 0.0, 0.0),
            _ => v,
        };
        if let Some(n) = v.normalized() {
            return n;
        }
    }
}

/// A start position clear of solid, near a random room centre.
fn random_start(lv: &Level, probe: &Oracle, rng: &mut Rng, r: f64) -> Option<Vec3d> {
    let c = *lv
        .centers
        .get(rng.below(lv.centers.len() as u32) as usize)?;
    for _ in 0..50 {
        let p = Vec3d::new(
            c.x + rng.range_f64(-18.0, 18.0),
            c.y + rng.range_f64(-18.0, 18.0),
            rng.range_f64(0.0, 12.0),
        );
        if !probe.overlaps(&lv.brushes, p, r + 0.01) {
            return Some(p);
        }
    }
    None
}

fn fail(what: &str, seed: u64, detail: &str) -> ! {
    panic!("{what} (level seed {seed}): {detail}");
}

fn fuzz_sweeps(n: usize) {
    for k in 0..LEVELS {
        let seed = 1000 + k as u64;
        let lv = make_level(seed);
        let probe = Oracle::new(&lv.brushes);
        let mut rng = Rng::new(seed ^ 0xABCD);
        for i in 0..n / LEVELS {
            let radius = RADII[rng.below(RADII.len() as u32) as usize];
            let r = f64::from(radius);
            let Some(mut from) = random_start(&lv, &probe, &mut rng, r) else {
                continue;
            };
            // Chain sweeps so later ones start in contact with a surface.
            for _ in 0..4 {
                let to = from + random_dir(&mut rng) * rng.range_f64(0.0, 14.0);
                let tr = sweep_sphere(&lv.bsp, from.to_f32(), to.to_f32(), radius);
                let end = tr.end.to_f64();
                let ctx = format!("iter {i} r {radius} from {from:?} to {to:?} trace {tr:?}");
                if probe.overlaps(&lv.brushes, end, r) {
                    fail("sweep ended inside solid", seed, &ctx);
                }
                if let Some(t) = probe.first_overlap(&lv.brushes, from, end, r, PATH_STEP) {
                    fail("sweep passed through solid", seed, &format!("t {t} {ctx}"));
                }
                if !tr.hit && (end - to).length() > 1e-4 {
                    fail("unobstructed sweep missed its target", seed, &ctx);
                }
                from = end;
            }
        }
    }
}

fn fuzz_slides(n: usize) {
    for k in 0..LEVELS {
        let seed = 2000 + k as u64;
        let lv = make_level(seed);
        let probe = Oracle::new(&lv.brushes);
        let mut rng = Rng::new(seed ^ 0x1234);
        for i in 0..n / LEVELS {
            let radius = RADII[rng.below(RADII.len() as u32) as usize];
            let r = f64::from(radius);
            let Some(start) = random_start(&lv, &probe, &mut rng, r) else {
                continue;
            };
            let (mut pos, mut vel) = (start.to_f32(), Vec3::ZERO);
            for frame in 0..30 {
                if frame % 10 == 0 {
                    vel = (random_dir(&mut rng) * rng.range_f64(0.0, 60.0)).to_f32();
                }
                let dt = rng.range_f32(0.002, 0.2);
                let res = slide_move(&lv.bsp, pos, vel, dt, radius);
                let ctx = format!(
                    "iter {i} frame {frame} r {radius} pos {pos:?} vel {vel:?} dt {dt} -> {res:?}"
                );
                if probe.overlaps(&lv.brushes, res.pos.to_f64(), r) {
                    fail("slide ended inside solid", seed, &ctx);
                }
                let budget = vel.length() * dt + 1e-3 * (MAX_BUMPS as f32 + 1.0);
                if (res.pos - pos).length() > budget {
                    fail("slide moved farther than allowed", seed, &ctx);
                }
                if res.contacts == 0 {
                    let (a, b) = (pos.to_f64(), res.pos.to_f64());
                    if probe
                        .first_overlap(&lv.brushes, a, b, r, PATH_STEP)
                        .is_some()
                    {
                        fail("free slide passed through solid", seed, &ctx);
                    }
                }
                pos = res.pos;
                vel = res.vel;
            }
        }
    }
}

#[test]
fn sweeps_never_enter_or_cross_solid() {
    fuzz_sweeps(iters());
}

#[test]
fn slides_never_end_inside_solid() {
    fuzz_slides(iters());
}

/// Regression (found by this fuzzer): the vertical jamb edge of a corridor
/// mouth belongs to an unbounded cell and was dropped because its ends touch
/// the clip box, so a sphere wider than the corridor slipped past it.
#[test]
fn regression_unbounded_cell_edge_is_swept() {
    let lv = make_level(1000);
    let tr = sweep_sphere(
        &lv.bsp,
        Vec3::new(30.0, 17.6, 3.0),
        Vec3::new(34.0, 17.6, 3.0),
        1.5,
    );
    // The corridor is 4 wide (y 15..19); a 1.5 radius sphere at y 17.6
    // overlaps the jamb at (32, 19) and must stop before it.
    assert!(tr.hit, "{tr:?}");
    assert!(tr.end.x < 32.0, "{tr:?}");
}
