//! M10.3: property test for movers. Random boxes translate and swing through
//! generated levels, with spheres flying around them. Size is set by
//! `MSCENT_FUZZ_MOVER_STEPS` (simulated steps per level).
//!
//! Judged by oracles that share no code with the collision under test: the
//! world by the ordered brush list, a mover by its box in a frame computed here
//! from the pose's own numbers. After every body move and every mover step:
//! * no sphere that started clear is inside the world or a mover;
//! * no sphere's flight crossed the world or a mover.

#[path = "fuzz/oracle.rs"]
mod oracle;

use mscent_collide::{Body, MoverShape, MoverSolid, Pose, Push, Solids, push_mover};
use mscent_math::rng::Rng;
use mscent_math::{Quat, Vec3, Vec3d};
use mscent_world::{Brush, Bsp, GenParams, Op, compile, generate, room_centers};
use oracle::Oracle;
use std::sync::Arc;

const DEFAULT_STEPS: usize = 4000;
const LEVELS: u64 = 8;
const BODIES: usize = 6;
const RADII: [f32; 5] = [0.1, 0.25, 0.5, 0.75, 1.0];
/// As the world oracle: a body wedged against something sits a skin off it,
/// and a few such presses can stack.
const TOLERANCE: f64 = 5.0e-3;
const PATH_STEP: f64 = 0.1;

/// The rotation part of a pose, applied the textbook way.
fn rotate(q: Quat, v: Vec3d) -> Vec3d {
    let u = Vec3d::new(f64::from(q.x), f64::from(q.y), f64::from(q.z));
    let w = f64::from(q.w);
    let n = (u.dot(u) + w * w).sqrt();
    let (u, w) = (u * (1.0 / n), w / n);
    let t = u.cross(v) * 2.0;
    v + t * w + u.cross(t)
}

fn to_local(pose: &Pose, world: Vec3d) -> Vec3d {
    rotate(pose.rot.conjugate(), world - pose.pivot - pose.offset) + pose.pivot
}

fn box_distance(lo: Vec3d, hi: Vec3d, p: Vec3d) -> f64 {
    let d = p.max(lo).min(hi) - p;
    d.length()
}

struct Mover {
    lo: Vec3d,
    hi: Vec3d,
    /// The pose it is at, and the two it travels between.
    now: Pose,
    a: Pose,
    b: Pose,
    s: f32,
    rate: f32,
    forward: bool,
    reverse_when_blocked: bool,
}

impl Mover {
    /// The middle of the box, where it is now.
    fn centre(&self) -> Vec3d {
        let c = self.lo.lerp(self.hi, 0.5);
        rotate(self.now.rot, c - self.now.pivot) + self.now.pivot + self.now.offset
    }

    fn overlaps(&self, p: Vec3d, r: f64) -> bool {
        box_distance(self.lo, self.hi, to_local(&self.now, p)) < r - TOLERANCE
    }

    fn step_target(&self) -> (f32, Pose) {
        let dir = if self.forward { 1.0 } else { -1.0 };
        let s = (self.s + self.rate * dir).clamp(0.0, 1.0);
        (s, Pose::between(&self.a, &self.b, s))
    }

    fn turn_round_at_ends(&mut self) {
        if self.s <= 0.0 {
            self.forward = true;
        } else if self.s >= 1.0 {
            self.forward = false;
        }
    }
}

struct World {
    brushes: Vec<Brush>,
    bsp: Bsp,
    oracle: Oracle,
    centers: Vec<Vec3d>,
}

fn make_world(seed: u64) -> World {
    let params = GenParams {
        cols: 1 + (seed % 3) as u32,
        rows: 1 + ((seed / 3) % 3) as u32,
        extra_links: (seed % 4) as u32,
        pillar_percent: 40 + (seed % 5) as u32 * 15,
    };
    let brushes = generate(seed, &params);
    let bsp = compile(&brushes).expect("generated level compiles");
    World {
        oracle: Oracle::new(&brushes),
        brushes,
        bsp,
        centers: room_centers(seed, &params),
    }
}

fn random_dir(rng: &mut Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
            rng.range_f32(-1.0, 1.0),
        );
        let v = match rng.below(3) {
            0 => Vec3::new(v.x.signum(), 0.0, 0.0),
            1 => Vec3::new(0.0, 0.0, v.z.signum()),
            _ => v,
        };
        if let Some(n) = v.normalized() {
            return n;
        }
    }
}

fn near_room(world: &World, rng: &mut Rng) -> Vec3d {
    let c = world.centers[rng.below(world.centers.len() as u32) as usize];
    Vec3d::new(
        c.x + rng.range_f64(-14.0, 14.0),
        c.y + rng.range_f64(-14.0, 14.0),
        rng.range_f64(1.0, 11.0),
    )
}

fn make_mover(world: &World, rng: &mut Rng) -> (Mover, MoverSolid) {
    let centre = near_room(world, rng);
    let half = Vec3d::new(
        rng.range_f64(0.2, 3.0),
        rng.range_f64(0.2, 3.0),
        rng.range_f64(0.2, 3.0),
    );
    let brush = Brush::cuboid(centre - half, centre + half, Op::Add, 0).expect("box");
    // The brush snaps to the grid, so its own bounds are the truth.
    let bounds = brush.bounds();
    let shape = Arc::new(MoverShape::from_brushes(std::iter::once(&brush)));
    let (rate, end) = if rng.below(2) == 0 {
        let travel = random_dir(rng).to_f64() * rng.range_f64(1.0, 12.0);
        (rng.range_f32(0.002, 0.2), Pose::translated(travel))
    } else {
        let pivot = centre + half;
        let angle = rng.range_f32(0.3, 3.0);
        let turn = Pose::turned(random_dir(rng), angle, pivot);
        (rng.range_f32(0.002, 0.5), turn)
    };
    let mover = Mover {
        lo: bounds.min.to_f64(),
        hi: bounds.max.to_f64(),
        now: Pose::IDENTITY,
        a: Pose::IDENTITY,
        b: end,
        s: 0.0,
        rate,
        forward: true,
        reverse_when_blocked: rng.below(2) == 0,
    };
    (mover, MoverSolid::new(shape))
}

/// Whether the sphere is clear of the movers' whole sweep at the start, so a
/// failure later is the code's, not the set-up's.
fn make_bodies(world: &World, movers: &[Mover], rng: &mut Rng) -> Vec<Body> {
    let mut bodies = Vec::new();
    for _ in 0..BODIES * 40 {
        if bodies.len() == BODIES {
            break;
        }
        let r = RADII[rng.below(RADII.len() as u32) as usize];
        let m = &movers[rng.below(movers.len() as u32) as usize];
        let p = m.centre() + random_dir(rng).to_f64() * rng.range_f64(0.5, 8.0);
        let clear = !world
            .oracle
            .overlaps(&world.brushes, p, f64::from(r) + 0.01)
            && !movers.iter().any(|m| m.overlaps(p, f64::from(r) + 0.01));
        if clear {
            bodies.push(Body::new(p.to_f32(), r));
        }
    }
    bodies
}

fn fail(what: &str, seed: u64, step: usize, detail: &str) -> ! {
    panic!("{what} (level seed {seed}, step {step}): {detail}");
}

fn check_body(world: &World, movers: &[Mover], body: &Body, seed: u64, step: usize, when: &str) {
    let (p, r) = (body.pos.to_f64(), f64::from(body.radius));
    if world.oracle.overlaps(&world.brushes, p, r) {
        fail(
            "body inside the world",
            seed,
            step,
            &format!("{when} {body:?}"),
        );
    }
    for (i, m) in movers.iter().enumerate() {
        if m.overlaps(p, r) {
            let d = box_distance(m.lo, m.hi, to_local(&m.now, p));
            let ctx = format!("{when} mover {i} depth {} {body:?} {:?}", r - d, m.now);
            fail("body inside a mover", seed, step, &ctx);
        }
    }
}

/// The straight flight of a body through a still world and still movers.
fn check_flight(
    world: &World,
    movers: &[Mover],
    (a, b): (Vec3d, Vec3d),
    r: f64,
    seed: u64,
    step: usize,
) {
    let n = ((b - a).length() / PATH_STEP).ceil().max(1.0) as u32;
    for i in 0..=n {
        let p = a.lerp(b, f64::from(i) / f64::from(n));
        // A slide bends, so only a move too short to bend is judged exactly.
        if movers.iter().any(|m| m.overlaps(p, r - 0.05)) {
            let ctx = format!("from {a:?} to {b:?} r {r} at {p:?}");
            fail("flight crossed a mover", seed, step, &ctx);
        }
    }
    if let Some(t) = world
        .oracle
        .first_overlap(&world.brushes, a, b, r - 0.05, PATH_STEP)
    {
        let ctx = format!("from {a:?} to {b:?} r {r} t {t}");
        fail("flight crossed the world", seed, step, &ctx);
    }
}

fn steer(bodies: &mut [Body], movers: &[Mover], rng: &mut Rng) {
    for body in bodies {
        let m = &movers[rng.below(movers.len() as u32) as usize];
        let aim = (m.centre() - body.pos.to_f64()).to_f32().normalized();
        let dir = match aim {
            Some(d) if rng.below(4) != 0 => d,
            _ => random_dir(rng),
        };
        body.vel = dir * rng.range_f32(0.0, 40.0);
    }
}

struct Run {
    pushes: u64,
    blocks: u64,
}

fn step_mover(
    world: &World,
    movers: &mut [Mover],
    solids: &mut [MoverSolid],
    i: usize,
    bodies: &mut [Body],
) -> Option<usize> {
    let (s, pose) = movers[i].step_target();
    let mut refs: Vec<&mut Body> = bodies.iter_mut().collect();
    match push_mover(&world.bsp, solids, i, &pose, &mut refs) {
        Push::Moved => {
            movers[i].s = s;
            movers[i].now = pose;
            movers[i].turn_round_at_ends();
            None
        }
        Push::Blocked(body) => {
            if movers[i].reverse_when_blocked {
                movers[i].forward = !movers[i].forward;
            }
            Some(body)
        }
    }
}

fn run_level(seed: u64, steps: usize) -> Run {
    let world = make_world(seed);
    let mut rng = Rng::new(seed ^ 0x90DE);
    let made: Vec<(Mover, MoverSolid)> = (0..4).map(|_| make_mover(&world, &mut rng)).collect();
    let (mut movers, mut solids): (Vec<Mover>, Vec<MoverSolid>) = made.into_iter().unzip();
    let mut bodies = make_bodies(&world, &movers, &mut rng);
    let mut run = Run {
        pushes: 0,
        blocks: 0,
    };
    for step in 0..steps {
        if step % 60 == 0 {
            steer(&mut bodies, &movers, &mut rng);
        }
        for body in &mut bodies {
            let before = body.pos.to_f64();
            body.step(Solids::new(&world.bsp, &solids), Vec3::ZERO);
            check_body(&world, &movers, body, seed, step, "after flight");
            let after = body.pos.to_f64();
            let r = f64::from(body.radius);
            check_flight(&world, &movers, (before, after), r, seed, step);
        }
        for i in 0..movers.len() {
            let before: Vec<Vec3> = bodies.iter().map(|b| b.pos).collect();
            if step_mover(&world, &mut movers, &mut solids, i, &mut bodies).is_some() {
                run.blocks += 1;
                let same = bodies.iter().zip(&before).all(|(b, p)| b.pos == *p);
                if !same {
                    fail("a blocked step moved a body", seed, step, "");
                }
            }
            if bodies.iter().zip(&before).any(|(b, p)| b.pos != *p) {
                run.pushes += 1;
            }
            for body in &bodies {
                check_body(&world, &movers, body, seed, step, "after mover");
            }
        }
    }
    run
}

#[test]
fn movers_never_leave_a_body_in_solid_or_through_a_wall() {
    let steps = std::env::var("MSCENT_FUZZ_MOVER_STEPS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(DEFAULT_STEPS);
    let (mut pushes, mut blocks) = (0, 0);
    for k in 0..LEVELS {
        let run = run_level(3000 + k, steps);
        pushes += run.pushes;
        blocks += run.blocks;
    }
    println!("{pushes} pushes, {blocks} blocked steps");
    // A test that never exercised the code proves nothing.
    assert!(pushes > 1000, "only {pushes} pushes");
    assert!(blocks > 100, "only {blocks} blocked steps");
}
