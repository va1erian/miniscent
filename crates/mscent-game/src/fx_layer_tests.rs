use super::*;
use crate::fx::budget::{Keep, Limits, Sink, Tally, cost_of, thin};
use crate::fx::class::Blend;
use crate::fx::combat::{CombatFx, DRONE_TINT};
use mscent_math::rng::Rng;
use mscent_math::{Vec3, Vec3d};
use mscent_render::particles::Mask;
use mscent_render::{Camera, Quality, Texture, Vertex, draw_polygon_flat};
use mscent_world::{Brush, Op, compile};

const W: f32 = 640.0;
const H: f32 = 360.0;
const BUDGET: f32 = 5.0 * W * H;

fn sprite(x: f32, radius: f32, tint: u32) -> SpriteDraw {
    SpriteDraw {
        x,
        y: H / 2.0,
        radius,
        inv_z: 0.5,
        tint,
        mask: Mask::Soft,
    }
}

fn tag(tier: Tier, key: u8) -> Tag {
    Tag { tier, key }
}

fn screen(depth: Option<&Framebuffer>) -> Screen<'_> {
    Screen {
        depth,
        width: W,
        height: H,
    }
}

/// `n` big sprites of a tier, "over" and additive alternately, 90 px wide.
fn big(n: usize, tier: Tier, draws: &mut ParticleDraws, tags: &mut Tags) {
    let mut sink = Sink {
        out: draws,
        tags,
        width: W,
        height: H,
    };
    for i in 0..n {
        let x = 100.0 + (i % 8) as f32 * 60.0;
        let blend = if i % 2 == 0 { Blend::Over } else { Blend::Add };
        let key = (i * 256 / n) as u8;
        sink.sprite(sprite(x, 45.0, 0x40ff_ffff), blend, tag(tier, key));
    }
}

fn lists() -> (ParticleDraws, Tags) {
    (ParticleDraws::default(), Tags::default())
}

fn apply(gate: &mut Gate, allowed: bool, over: bool, d: &mut ParticleDraws, t: &mut Tags) -> bool {
    gate.apply((allowed, over), (d, t), &screen(None))
}

/// A target with a wall at `1/z = 0.9`, nearer than the test sprites.
fn walled() -> Framebuffer {
    let cam = Camera::look_at(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 10.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        1.2,
        0.1,
        100.0,
        640,
        360,
    )
    .expect("camera");
    let tex = Texture::new(16, 16, vec![0xff80_8080; 256]).expect("texture");
    let v = |x: f32, z: f32| Vertex::new(Vec3::new(x, 1.1, z), 0.0, 0.0);
    let mut fb = Framebuffer::new(640, 360).expect("target");
    let quad = [
        v(-50.0, -50.0),
        v(50.0, -50.0),
        v(50.0, 50.0),
        v(-50.0, 50.0),
    ];
    draw_polygon_flat(&mut fb, &cam, &quad, &tex, 256, None);
    assert!(
        fb.depth_at(320, 180).is_some_and(|z| z > 0.8),
        "the wall is near"
    );
    fb
}

#[test]
fn big_normal_and_cosmetic_sprites_move_when_over_budget_and_the_rest_stay() {
    let (mut d, mut t) = lists();
    big(10, Tier::Normal, &mut d, &mut t);
    big(10, Tier::Cosmetic, &mut d, &mut t);
    big(4, Tier::Essential, &mut d, &mut t);
    // Small ones: 60 px across is under the 64 px line.
    let mut sink = Sink {
        out: &mut d,
        tags: &mut t,
        width: W,
        height: H,
    };
    // Nearer than all of them: one behind would go too, to keep its order.
    let near = SpriteDraw {
        inv_z: 0.9,
        ..sprite(300.0, 30.0, 0x40ff_ffff)
    };
    sink.sprite(near, Blend::Over, tag(Tier::Normal, 1));
    sink.sprite(
        sprite(300.0, 32.0, 0x40ff_ffff),
        Blend::Add,
        tag(Tier::Normal, 2),
    );
    let before = cost_of(&d, None, W, H);
    let mut gate = Gate::default();
    assert!(apply(&mut gate, true, true, &mut d, &mut t));
    assert_eq!(d.layer_over.len() + d.layer_add.len(), 20);
    assert_eq!(d.layer_over.len(), t.layer_over.len());
    assert_eq!(d.layer_add.len(), t.layer_add.len());
    // The small two and the four Essential ones stay.
    assert_eq!(d.over.len() + d.add.len(), 6);
    assert!(
        t.over
            .iter()
            .chain(&t.add)
            .all(|t| t.tier != Tier::Normal || t.key < 3)
    );
    assert!(
        t.layer_over
            .iter()
            .chain(&t.layer_add)
            .all(|t| t.tier != Tier::Essential)
    );
    assert!(cost_of(&d, None, W, H) < before, "and the frame is cheaper");
}

#[test]
fn nothing_moves_under_budget_or_when_the_quality_forbids_it() {
    let (mut d, mut t) = lists();
    big(12, Tier::Normal, &mut d, &mut t);
    let mut gate = Gate::default();
    assert!(
        !apply(&mut gate, true, false, &mut d, &mut t),
        "under budget"
    );
    assert!(
        !apply(&mut gate, false, true, &mut d, &mut t),
        "not allowed"
    );
    assert!(d.layer_over.is_empty() && d.layer_add.is_empty());
    assert_eq!(gate.held(), 0);
    assert!(Limits::of(Quality::High).layer && Limits::of(Quality::Medium).layer);
    assert!(!Limits::of(Quality::Low).layer);
}

#[test]
fn the_layer_is_used_only_when_it_lowers_the_total() {
    let mut rng = Rng::new(0x1a7e5);
    let (mut used, mut refused) = (0, 0);
    for _ in 0..200 {
        let (mut d, mut t) = lists();
        let mut sink = Sink {
            out: &mut d,
            tags: &mut t,
            width: W,
            height: H,
        };
        for _ in 0..rng.below(24) {
            let r = rng.range_f32(5.0, 120.0);
            let s = SpriteDraw {
                x: rng.range_f32(-50.0, W + 50.0),
                y: rng.range_f32(-50.0, H + 50.0),
                ..sprite(0.0, r, 0x40ff_ffff)
            };
            let blend = if rng.below(2) == 0 {
                Blend::Over
            } else {
                Blend::Add
            };
            let tier = [Tier::Essential, Tier::Normal, Tier::Cosmetic][rng.below(3) as usize];
            sink.sprite(s, blend, tag(tier, rng.below(256) as u8));
        }
        let before = cost_of(&d, None, W, H);
        if apply(&mut Gate::default(), true, true, &mut d, &mut t) {
            used += 1;
            assert!(cost_of(&d, None, W, H) < before, "moved, so cheaper");
        } else {
            refused += 1;
            assert_eq!(cost_of(&d, None, W, H), before, "left alone");
        }
    }
    assert!(
        used > 20 && refused > 20,
        "both happen: {used} and {refused}"
    );
}

#[test]
fn a_sprite_the_scene_hides_stays_at_full_size() {
    let wall = walled();
    let (mut d, mut t) = lists();
    big(16, Tier::Normal, &mut d, &mut t);
    let mut gate = Gate::default();
    // The sprites are at 1/z 0.5, behind a wall at 0.9.
    assert!(!gate.apply((true, true), (&mut d, &mut t), &screen(Some(&wall))));
    assert!(d.layer_over.is_empty() && d.layer_add.is_empty());
    // The same sprites in the open do move.
    assert!(gate.apply((true, true), (&mut d, &mut t), &screen(None)));
}

#[test]
fn moving_keeps_the_order_and_costing_stays_consistent() {
    let (mut d, mut t) = lists();
    big(16, Tier::Cosmetic, &mut d, &mut t);
    let order: Vec<f32> = d.over.iter().map(|s| s.x).collect();
    let mut gate = Gate::default();
    let before = Tally::of(&d, &t, None, W, H);
    assert!(before.over_budget(0.0));
    assert!(apply(&mut gate, true, true, &mut d, &mut t));
    let moved: Vec<f32> = d.layer_over.iter().map(|s| s.x).collect();
    assert_eq!(moved, order, "back-to-front order is the list's");
    // A moved draw is costed as a layer draw, in its own tier, plus the
    // layer's fixed cost once: the tally reaches the same total as `cost_of`,
    // which the cut is computed from.
    let after = Tally::of(&d, &t, None, W, H);
    let total = cost_of(&d, None, W, H);
    assert!(total < cost_of_flat(&d), "cheaper than at full size");
    assert!(after.over_budget(total - 1.0) && !after.over_budget(total + 1.0));
}

/// The cost of the same draws all at full size.
fn cost_of_flat(d: &ParticleDraws) -> f32 {
    let mut flat = d.clone();
    flat.over.splice(0..0, flat.layer_over.drain(..));
    flat.add.append(&mut flat.layer_add);
    cost_of(&flat, None, W, H)
}

#[test]
fn thin_cuts_and_fades_layered_draws_like_any_other() {
    let (mut d, mut t) = lists();
    big(16, Tier::Cosmetic, &mut d, &mut t);
    assert!(apply(&mut Gate::default(), true, true, &mut d, &mut t));
    let layered = d.layer_over.len() + d.layer_add.len();
    let keep = Keep {
        normal: 300,
        cosmetic: 128,
    };
    thin(&mut d, &t, keep);
    let left = d.layer_over.len() + d.layer_add.len();
    assert!(left > 0 && left < layered, "{left} of {layered}");
    assert_eq!(d.layer_over.len() + d.layer_add.len(), left);
    // The one nearest the cut is faded, not at full tint.
    let faded = d
        .layer_over
        .iter()
        .chain(&d.layer_add)
        .any(|s| s.tint != 0x40ff_ffff);
    assert!(faded, "a fade band sits just above the cut");
    let gone = Keep {
        normal: 0,
        cosmetic: 0,
    };
    thin(&mut d, &t, gone);
    assert!(d.layer_over.is_empty() && d.layer_add.is_empty());
    assert_eq!(cost_of(&d, None, W, H), 0.0, "no layer, no fixed cost");
}

#[test]
fn the_layer_is_held_for_a_count_of_frames_and_then_released() {
    let mut gate = Gate::default();
    let frame = |gate: &mut Gate, over: bool| {
        let (mut d, mut t) = lists();
        big(16, Tier::Normal, &mut d, &mut t);
        apply(gate, true, over, &mut d, &mut t)
    };
    assert!(frame(&mut gate, true));
    assert_eq!(gate.held(), HOLD_FRAMES);
    // The scene dips under budget: the layer stays, so the smoke does not
    // pop between half and full size on adjacent frames.
    for i in 0..HOLD_FRAMES {
        assert!(frame(&mut gate, false), "held on frame {i}");
    }
    assert!(!frame(&mut gate, false), "and then released");
    // Needing it again starts a fresh hold.
    assert!(frame(&mut gate, true));
    assert_eq!(gate.held(), HOLD_FRAMES);
    gate.reset();
    assert!(!frame(&mut gate, false));
}

#[test]
fn hostile_sprites_stay_put_and_nothing_panics() {
    let (mut d, mut t) = lists();
    big(16, Tier::Normal, &mut d, &mut t);
    for radius in [f32::NAN, f32::INFINITY, -1.0, 0.0, f32::MAX] {
        for tier in [Tier::Normal, Tier::Cosmetic] {
            d.over.push(sprite(f32::NAN, radius, 0xffff_ffff));
            t.over.push(tag(tier, 0));
            d.add.push(sprite(300.0, radius, 0xffff_ffff));
            t.add.push(tag(tier, 0));
        }
    }
    let mut gate = Gate::default();
    apply(&mut gate, true, true, &mut d, &mut t);
    assert!(cost_of(&d, None, W, H).is_finite());
    let tally = Tally::of(&d, &t, None, W, H);
    assert!(tally.fit(BUDGET).normal <= 288);
}

/// Forty drone explosions before the camera, as in `fx_budget_bench`, settled
/// into the smoke.
fn heavy(layer: bool) -> (ParticleDraws, Keep) {
    let room = Brush::cuboid(
        Vec3d::new(-40.0, -40.0, -40.0),
        Vec3d::new(40.0, 40.0, 40.0),
        Op::Subtract,
        0,
    )
    .expect("a room");
    let bsp = compile(&[room]).expect("compiles");
    let view = mscent_render::View::from_angles(Vec3::ZERO, 0.0, 0.0, 0.0);
    let cam = view.camera(1.3, 0.1, 400.0, 640, 360).expect("a camera");
    let mut fx = CombatFx::new();
    fx.set_layer(layer);
    for i in 0..40 {
        let spread = i as f32 / 40.0 - 0.5;
        let at = Vec3::new(spread * 16.0, 8.0 + spread * 4.0, spread * 6.0);
        fx.drone_explosion(at, DRONE_TINT);
    }
    for _ in 0..12 {
        fx.advance(16_666_667, &bsp);
    }
    let mut draws = CombatFx::new_draws();
    // Frames, so the cut has settled from its start.
    for _ in 0..8 {
        fx.collect(&cam, None, None, &mut draws);
    }
    (draws, fx.keep())
}

#[test]
fn a_heavy_scene_stays_in_budget_and_keeps_more_cosmetic_smoke_with_the_layer() {
    let (with, keep_with) = heavy(true);
    let (without, keep_without) = heavy(false);
    let layered = with.layer_over.len() + with.layer_add.len();
    assert!(layered > 20, "the layer is used: {layered}");
    assert!(without.layer_over.is_empty() && without.layer_add.is_empty());
    let budget = Limits::of(Quality::High).screens * W * H;
    for (name, d) in [("with", &with), ("without", &without)] {
        let cost = cost_of(d, None, W, H);
        assert!(cost <= budget * 1.1, "{name}: {cost} against {budget}");
    }
    assert!(
        keep_with.cosmetic > keep_without.cosmetic,
        "cosmetic cut {} against {}",
        keep_with.cosmetic,
        keep_without.cosmetic
    );
    let sprites =
        |d: &ParticleDraws| d.over.len() + d.add.len() + d.layer_over.len() + d.layer_add.len();
    assert!(
        sprites(&with) > sprites(&without),
        "{} against {}",
        sprites(&with),
        sprites(&without)
    );
}

#[test]
fn small_smoke_behind_the_nearest_big_puff_goes_with_it_and_nearer_smoke_stays() {
    let (mut draws, mut tags) = (ParticleDraws::default(), Tags::default());
    let puff = |x: f32, radius: f32, inv_z: f32| SpriteDraw {
        x,
        y: 180.0,
        radius,
        inv_z,
        tint: 0x8040_4040,
        mask: Mask::Soft,
    };
    let tag = Tag {
        tier: Tier::Cosmetic,
        key: 0,
    };
    let mut sink = Sink {
        out: &mut draws,
        tags: &mut tags,
        width: 640.0,
        height: 360.0,
    };
    sink.sprite(puff(300.0, 200.0, 0.5), Blend::Over, tag);
    sink.sprite(puff(300.0, 10.0, 0.1), Blend::Over, tag);
    sink.sprite(puff(300.0, 10.0, 0.9), Blend::Over, tag);
    let screen = Screen {
        depth: None,
        width: 640.0,
        height: 360.0,
    };
    let moved = Gate::default().apply((true, true), (&mut draws, &mut tags), &screen);
    assert!(moved);
    let inv_z = |list: &[SpriteDraw]| list.iter().map(|s| s.inv_z).collect::<Vec<_>>();
    assert_eq!(
        inv_z(&draws.layer_over),
        [0.5, 0.1],
        "in the order they came"
    );
    assert_eq!(inv_z(&draws.over), [0.9]);
}
