# Particles: combat effects in a software renderer

M9.0 to M9.4 are built; the rest is design. It replaces the screen-space tracer that was in
[`apps/game/src/feedback.rs`](../apps/game/src/feedback.rs) with a world-space particle system
that covers the shots, hits and explosions of combat and can later serve a destructible brush.
See [What was built](#what-was-built-m91-to-m93) for where the code differs from this design.

## What was there before

- **Enemy shots.** `Feedback` draws a dotted 2D line from the projected enemy to a fixed screen
  point (`w/2, h*0.62`), plus a square muzzle flash. Both ignore depth, so they show through walls,
  and neither knows where the shot went.
- **Player shots.** A HUD rectangle at the bottom of the screen and nothing in the world. The hit
  point is computed in `Weapon::fire` and thrown away.
- **Kills.** The enemy stops being drawn on the step it dies (`actors()` filters `alive()`), and
  the word `KILL` appears.
- **What the game does not report.** `Hitscan` has a point but no normal. `Enemy::attack` drops its
  trace entirely. `Event::EnemyHit(i)` has no position. Nothing maps a hit to a material.

Three facts about the renderer shape the design:

- `Framebuffer` keeps its `1/z` depth after `LevelRenderer::render`, so a later pass can depth test
  against the finished scene.
- The game renders in parallel bands; see [Threading](#threading). The measurements below were
  taken on one thread, before that change.
- The project treats pixels as reproducible for a given script (`app.rs` keeps wall-clock numbers
  out of unpaced runs). Effects have to preserve that.

## Measurements

These come from a throwaway benchmark on the real showcase frame: 640x360, Quality High, one
thread, baseline x86-64 (SSE2), on the dev machine (Core Ultra 7 270K Plus). The kernels were
prototypes. M9.1 lands them with a bench in the repo, and the numbers should be redone there.

| Operation | Cost on dev machine |
|---|---|
| Scene frame, Low / Medium / High | 5.2 / 8.1 / 8.9 ms |
| Additive sprite, plain Rust, per bbox pixel | 1.0 to 1.2 ns |
| Additive sprite, SSE2 | 0.13 to 0.18 ns |
| Premultiplied "over" sprite, SSE2 | 0.39 to 0.43 ns |
| Sprite fully behind the depth buffer, SSE2 | 0.03 to 0.06 ns |
| Nearest-scale row fetch through a u-table | 0.24 ns |
| Spark streak, 4 / 16 / 64 px | 12 / 36 / 126 ns (about 2.3 ns/px) |
| Simulate 4096 particles (SoA Euler, drag, gravity) | 5.1 us (1.2 ns each) |
| Project 4096 points through `Camera` | 12 us (3 ns each) |
| `Bsp::is_solid` (84 nodes) | 22 ns |
| `sweep_sphere`, radius 0, 0.4 units / 36 units | 222 ns / 23.9 us |
| 16x16 coarse depth grid over the screen | 42 us |
| Half-res layer: depth downsample / composite, empty to 360 px covered | 64 us / 13 to 43 us |
| Screen-space flash from a 16 px depth grid, scalar | 221 us |

Sprites under 16 px fell below the timer's noise, so per-sprite setup cost is **not** measured.

What these numbers decide:

1. **Fill is the only real cost.** 4096 particles simulate and project in under 20 us. A single
   full-screen "over" sprite costs about 100 us. Effort goes into pixels, not particle counts.
2. **SIMD is worth its `unsafe`.** SSE2 is 6 to 9 times faster than the plain Rust loop.
3. **Per-particle traces are impossible.** One long ray costs as much as 1,100 `is_solid` calls.
   Particles get a cheap point test, and a short sweep only when they actually enter a wall.
4. **No hierarchical depth.** Occluded sprites are already 3 to 8 times cheaper than visible ones
   because the kernel exits on a 4-pixel mask. A grid at 42 us only pays once more than three
   occluded full-screen sprites exist per frame.
5. **The half-res layer is not free.** Its fixed cost is about 100 us, the price of about one
   full-screen sprite. It wins only under heavy overdraw, so it is off unless a frame needs it.

### The reference machine

No number exists for the i3-7100U (M1.4 is not done). This doc plans with **5 times the dev
machine**, from the plan's own estimate (0.94 ms becoming 4 to 5 ms). That is a guess until M8.5.

At that factor the scene alone is about 40 ms at High on one worker and about 20 ms on two, so the
resolution controller would still be below full size there. Effects cannot wait for spare time.
They get a fixed share, and that share scales with the pixel count so it stays the same fraction at
any internal resolution.

## Threading

**Built (M9.0).** `LevelRenderer::render` now draws in parallel bands by default, with
`available_parallelism().max(4)` workers on a pool of parked threads. A frame is:

```
serial    portal flood, BSP walk, clip and project every face and mesh triangle, once
parallel  bands: sky, level, meshes (a band fills only the polygons in its rows)
serial    (M9.2 onward: particle collect and budget, reading depth from the bands)
parallel  (M9.1 onward: bands: particles)
serial    composite, then HUD
```

Measured on the showcase at High, 640x360, 8 views with actors, best of 7. The pinned columns are
this machine held to 1 and 2 of its cores, not the i3's two cores and two hyperthreads, so they only
show the shape.

| Bands, workers | All cores | Pinned to 2 cores | Pinned to 1 core |
|---|---|---|---|
| One full-height band, 1 | 8.26 ms | 8.30 ms | 8.00 ms |
| 24 rows, 2 | 4.12 ms | 4.04 ms | 8.51 ms |
| 24 rows, 4 | 2.16 ms | 4.07 ms | 8.46 ms |
| 24 rows, 12 | 1.15 ms | 4.93 ms | 8.44 ms |
| 24 rows, 24 | 0.98 ms | 5.01 ms | 10.08 ms |

The game's own bench went from 9.29 to 1.20 ms per frame (p95 1.36 ms). Allocations are the same as
the single-threaded renderer's: 256 per frame without actors, 1379 against 1382 with them. A first
trial that simply ran the old frame in each band cost 2082 and 1.42 ms, because every band repeated
the flood, the walk and the clipping of every polygon.

**The pool** is a parallel-for and nothing more, in `crates/mscent-render/src/jobs.rs`:

```rust
impl JobPool {
    /// `workers - 1` parked threads; one worker runs everything on the caller, with no threads.
    pub fn new(workers: usize) -> Self;
    /// Runs `f(0..jobs)` on the pool and the caller; returns when all are done.
    pub fn run<F: Fn(usize) + Sync>(&mut self, jobs: usize, f: &F);
}
```

One `unsafe` block lends the borrowed closure to the parked threads, sound because `run` waits until
none can still reach it. A panic in a job is caught and raised on the caller, so the editor still
catches a scene panic. There is no task graph, priority or queue, since nothing needs them. Compared
with spawning, with threads idle 5 ms before each dispatch (a prototype pool; median and p95):

| Helpers | `thread::scope` spawn | Parked pool |
|---|---|---|
| 3 | 181 / 242 us | 22 / 51 us |
| 7 | 322 / 442 us | 30 / 61 us |
| 23 | 803 / 1288 us | 57 / 102 us |

**What is threaded in the particle system, and what is not:**

| Work | Cost | Threaded? |
|---|---|---|
| Simulate 4096 | 5 us | No. Dispatch alone costs 22 us |
| Project and collect 4096 | 12 us | No, for the same reason; it also has to run after the scene pass |
| Sprite, spark and beam fill | up to the whole budget | **Yes**, per band |
| Half-res layer | fill and composite | **Yes**, per band; a layer row belongs to the band of its full-res rows, so band height stays even |

Fill is the only real cost, and it is the part bands already split. Each band clips the same
read-only draw list to its rows, so no sort or list is rebuilt per band and the output does not
depend on the worker count. `TileRenderer::pass` can be called again before `composite`, which is
the slot the particle pass takes.

**Worker count.** A minimum of 4 costs nothing on 2 cores and about 6% against one band on 1 core
(8.00 to 8.46 ms, close to the noise of these runs). That matters on LazyOS, where a kernel without
`LAZYOS_SMP=1` runs one core, and where threads in user code are still unproven (M1.3). The plan
requires a working one-worker path there, so the minimum belongs to the default, and
`LevelRenderer::set_tiling` is the override the LazyOS backend passes; no backend passes it yet.

## What was built (M9.1 to M9.3)

`crates/mscent-render/src/particles/` draws and `crates/mscent-game/src/fx/` simulates, as designed.
There is no dynamic light yet (M9.5).

Measured with `cargo run --release -p mscent-render --example fx_bench` (640x360, one thread, dev
machine). These replace the prototype figures in the table above where they differ:

| Operation | Scalar | SSE2 | AVX2 |
|---|---|---|---|
| Additive sprite, ns per bbox pixel | 1.34 to 1.39 | 0.54 to 0.56 | 0.45 |
| "Over" sprite, ns per bbox pixel | 1.79 to 1.88 | 0.67 to 0.70 | 0.53 |
| Fully occluded sprite, ns per bbox pixel | 0.08 | 0.08 | 0.08 |

Visible fill is about three times the prototype additive figure and 1.7 times its "over" figure,
because these numbers include fetching the mask through the u-table. The budget weights in M9.4
should come from this table. A spark costs 45 / 113 / 367 ns at 4 / 16 / 64 px, width 1 (about 5.7
ns/px at 64 px, against the prototype's 2.3).

**Sprite setup, the open risk from the design, is real.** A 2 px sprite costs 42 ns and a 16 px one
375 ns, about 35 ns plus 20 ns per row. 4096 sprites of 2 to 4 px would cost about 0.2 to 0.3 ms, so
M9.4 should cost small sprites by count as well as area, or draw them as points.

The simulation ticks 4096 uncollided particles in 21 us, about 4 times the prototype, because each
particle also has validity checks and a world bound. At the collision limits, about 3200 live
particles of mixed classes tick in 56 us, inside the 90 us the design allowed.

Where the code differs from the design:

- **Bounce uses `is_solid` bisection, not `sweep_sphere`.** `sweep_sphere` allocates on every call,
  which would break the zero-allocation promise. A bounce bisects the step 20 times and takes the
  normal from six axis probes: exact for axis-aligned and 45 degree walls, coarse for other slopes.
  Each query counts as one of the 32 "sweeps"; its extra `is_solid` calls (up to 28) do not count
  against the 512.
- **Over the test limit a bouncing particle dies** instead of skipping the test, since skipping
  could leave it in solid. Bouncing classes step first, so this needs more than 512 of them.
- **`Scene::material_at` became `Scene::surface_at`** and returns the material and its mean colour.
- **Chips and shards are small soft sprites** until polygon shards arrive with M9.6.
- **The drone explosion's tables landed in M9.3**, ahead of M9.6, because every primitive it needs
  exists and a kill otherwise just removes the mesh. It has no light yet, and its smoke is not lit.
- **Particles draw in the scene's band pass**, not a second one, which saves a dispatch. M9.2's
  collect runs before the frame, so M9.4's depth probes will need a pass of their own.
- **A hit on the player spawns no impact**, since it would burst inside the camera.
- **`Enemy::step` clears `last_shot` before its alive check.** The old `fired` flag stayed set on an
  enemy killed the step after it fired, so it reported `EnemyFired` on every later step.

Evidence: `particles/*_tests.rs`, `fx_tests.rs`, `fx_collide_tests.rs`, `fx_combat_tests.rs`,
`surface_tests.rs`, `tests/fx_alloc.rs`, `tests/fx_collect_alloc.rs` and `apps/game/tests/combat_fx.rs`,
which draws through a real baked level: a shot behind a wall changes no pixels (the same effects in
the open change more than 100), metal and brick give different pictures, and a scripted shot is
pixel-identical twice. Screenshots of a kill and a close wall hit were looked at during development.
Not tested: a real window, the i3-7100U, or whether the recipes look right to a person who plays it.

## What was built (M9.4)

`crates/mscent-game/src/fx/budget.rs` fits each frame to the budget during collect: it costs every
draw, cuts Cosmetic and then Normal by key, and never touches Essential. Measured with
`cargo run --release -p mscent-game --example fx_budget_bench` (40 drone explosions, 640x360, one
thread, AVX2 on the dev machine, no scene behind them):

| Quality | Spent of budget, screens | Sprites, streaks | Collect | Draw | ns per unit |
|---|---|---|---|---|---|
| High | 4.82 of 5.0 | 76, 149 | 15 us | 0.48 ms | 0.43 |
| Medium | 2.94 of 3.0 | 55, 116 | 16 us | 0.28 ms | 0.41 |
| Low | 1.21 of 1.5 | 37, 145 | 18 us | 0.12 ms | 0.40 to 0.43 |

The ns per unit holding steady across all three, close to the 0.45 ns of an AVX2 additive pixel, is
what says the weights are about right. At that rate High's 5 screens are about 0.5 ms on the dev
machine, or 2.5 ms on the reference at the 5x guess. This is not a run on the i3-7100U.

Where the code differs from the design below:

- **The weights are the measured ones**: "over" 1.25, streak pixels 10, plus 64 per sprite and 36 per
  row, which covers the small-sprite setup M9.1 found. The design's 1.7 and 5.8 came from prototypes.
- **Depth probes read the last frame.** Collect runs before the scene is drawn, because particles
  share its band pass, and a second dispatch would cost more than the probes save. A wrong probe
  charges the wrong amount for one frame and changes no pixel beyond the thinning it causes.
- **`keep` falls at once and climbs over about a second**, rather than a two-way low-pass, so the
  budget holds on the frame of the spike. `keep` moves per frame, so it follows the frame rate.
- **The first cut can pop slightly.** Going from nothing thinned to a cut puts keys inside the fade
  at once. Not yet seen in a picture.
- **Spawn density is by key, with no distance rule.** Quality thins Normal and Cosmetic at birth.
  Fewer spawns at a distance and none beyond fog are not built: collect already drops fogged
  particles, so that saves simulation only, which costs little.

Not built, and needed before this design is finished:

- **The adaptive multiplier.** The particle pass runs inside the scene's bands, so nothing times it
  apart from the scene yet.
- **Hiding particle cost from the resolution controller**, for the same reason. Until then a big
  explosion can lower the internal resolution for a while after it ends. Not measured.
- **Moving large sprites to the half-res layer** arrives with the layer, in M9.6.

## What was built (M9.5)

A face within reach of a light and facing it gets the lit variant of `LightingShader`, as designed;
every other face runs the code it ran before. `DynLight` (`dynlight.rs`) is a point light that has
been placed in a level: `place` floods the portals from its leaf once and keeps the leaves and the
faces it reaches (`Scene::light_reach`, `scene/reach.rs`), so a frame tests one bit per face. The
renderer takes the lights as a read-only slice, `LevelRenderer::render_with_effects(.., &Effects {
particles, lights })`, and `render_with_particles` is that with no lights. `CombatFx` owns the
lights (`fx/lights.rs`), fades them on the particle clock, and hands them out with
`CombatFx::lights(scene)`. `Game` never sees them.

Measured with `cargo run --release -p mscent-render --example light_bench` (new; 640x360, one
thread, High, dev machine). The camera is 3 units from a wall that fills the frame, which is 14400
spans, with 1, 2 or 4 lights in front of it; the figures are the frame time with the lights minus
the frame time without, and the same per span:

| Lights | Bright (colour 1.0, 0.8, 0.5) | Dim (5% of that) |
|---|---|---|
| none | 5.76 ms | 5.80 ms |
| 1 | +0.28 ms, 19 ns per span | +0.21 ms, 15 ns per span |
| 2 | +0.63 ms, 44 ns per span | +0.26 ms, 18 ns per span |
| 4 | +0.98 ms, 68 ns per span | +0.35 ms, 25 ns per span |

**The estimate of 100 to 150 us was wrong.** One light across the whole screen is 0.2 to 0.3 ms,
about twice that, and four bright ones are about 1 ms. The dim column is the light's own cost: the
sum at both span ends and the add across the span. The bright column pays more because the sum
pushes pixels over the knee of `soft_limit`, which divides three times per pixel (baked bright
pools pay that too). I did not run it with the roll-off removed, so that split is an inference. At
the 5x guess for the reference machine this is 1.4 ms for one light and 5 ms for four bright ones,
in the worst case of every pixel on screen lit. A flash covers part of the view for 0.1 to 0.25 s,
so a frame is usually far below that, but the case is not measured. The screen-space fallback was
221 us for all lights, so at four bright lights over the whole screen this is about four times
worse; I did not switch to it, and M9.6 should look at the peak frame of `Blast` before deciding.

**No cost with no light.** Every number here is the dev machine, and the runs were interleaved with
a build of 7b4692d made for the purpose:

- `light_bench --lights 0`, which goes through `LevelRenderer` and the lightmap shader: 5.768,
  6.094, 6.679 and 5.798 ms at 7b4692d against 5.745, 6.318, 5.942 and 5.824 ms now (four pairs,
  best of nine batches each; the spread is the machine's).
- `frame_bench` (`cargo run --release -p mscent-render --example frame_bench -- --batches 11`),
  three pairs: 1.423, 1.424, 1.426 ms before and 1.446, 1.438, 1.435 ms after, about 1% slower in
  every pair. That bench draws through `Scene::render` and calls none of the code changed here, so
  this is not a shader cost, but I did not find what it is.
- The scripted flythrough PNGs at 2, 12 and 24 s (`--screenshot .. --script
  apps/game/scripts/flythrough.txt --shots-at 2,12,24`) are byte-identical to 7b4692d's.
- Allocations: 478 a frame at 160x90 on one worker with one drone, before, with no light, and with
  two lights alive (`tests/dyn_light_alloc.rs`). Placing a second light into a `DynLight` that
  held one allocates nothing.

Where the code differs from the design above:

- **The flood runs when the light is first drawn, not at the spawn.** A spawn has no scene (bolts
  land inside `CombatFx::advance`, which holds only the BSP), so `CombatFx::lights(scene)` places
  new lights, once each. It is per light, not per frame, and the bit sets are reused between
  lights. The flood crosses a portal only if the light is on the right side of it and the portal's
  box is within the radius; each leaf is visited once. A light in solid reaches nothing.
- **A face is marked as well as a leaf**: in a reached leaf, in front of the light, and within the
  radius of its plane. The frame never looks at leaves for walls.
- **Meshes are tested by the leaf their centre is in**, so a drone beside a wall from a light is
  not lit through it (`an_enemy_behind_a_wall_from_the_light_is_not_lit`). A mesh sees one
  brightness, the mean of the colour, as the level's own lights do.
- **A flash on a wall is lifted off it**: 0.6 units along the normal (0.5 for a drone), halved
  twice if that lands in solid. At 0.05 off, `N.L` against that wall is near zero.
- **Falloff is the mesh formula**, `(1 - d / radius)^2 * N.L`. A colour of 1.0 adds one plain
  texture colour at point-blank range; the roll-off ends at about 1.17 of the texture, so a flash
  cannot blow a wall out.
- **Bump follows the baked light.** The dynamic light joins `col[]` before the bump gain, which is
  computed from the baked light direction, so relief under a flash does not follow the flash.
  Specular is not driven by it. Faces without a lightmap (light panels) take no dynamic light.
- **Over the limit the dimmest gives way**, by current strength (colour sum times the fade), and
  the new light competes: one dimmer than every light alive is dropped. `set_quality` drops the
  dimmest at once when the limit falls.
- **Lights age on the particle clock** with the same cap of four ticks per call, and fade as the
  square of the time left. The session does not advance effects while paused, so lights freeze
  with the particles. That is tested on `CombatFx` (`advance(0)` changes nothing) and not through a
  paused `GameSession`.
- **Recipes**: drone explosion radius 12, 0.25 s, colour (3.0, 1.8, 0.9); surface hit radius 4,
  0.1 s, (1.0, 0.8, 0.5); drone hit radius 4, 0.1 s, (0.6, 0.95, 1.1). The explosion started at
  1.5 and was doubled because the walls of the showcase room, 5 or more units from it, hardly
  changed in a screenshot. At 3.0 the largest change of any channel in that frame (an explosion 3
  units ahead of the camera, particles left out) is 19 of 255. No player has judged any of this,
  and the effect on far brick is still subtle.
- **Not built**: bolt-head lights, `Blast`, the half-res layer, shake, overlay (M9.6).

Evidence: `face_points_tests.rs` (the formula, the four-light cap, hostile values, span steps),
`dynlight_tests.rs` (reach through a split room and a corridor, the radius bound, hostile
values), `fx_lights_tests.rs` (fade, limits, eviction, lift, replay), `tests/dyn_light_alloc.rs`,
and `apps/game/tests/dynamic_lights.rs` on real pixels: a light brightens the wall facing it and
changes nothing behind the viewer; a wall behind a wall is unchanged to the pixel, while the same
light on the near side changes more than 10000 pixels; a mesh is lit and nothing outside it moves;
a light that has faded out or never lit changes no pixel and matches `render_with_particles`;
hostile lights do not panic; 1, 2, 3 and 8 workers give identical pixels with a light, two drones
and an explosion's particles; the recipes spawn the lights above, at the arrival of the bolt and
not at the muzzle. Mutating the shader or the mesh path to ignore the lights made the matching
tests fail. Not tested: a real window, the i3-7100U, or a kill through a full session with the
light on screen.

## Design overview

```
Game::step ──events──▶ GameSession ──Effect + args──▶ Fx (mscent-game)
 (deterministic,        resolves material,             class pools, 60 Hz sim,
  knows nothing of       owns the budget                collision, emission
  particles)                                                  │
                                                              │ collect: project, cull, cost
                                                              ▼
LevelRenderer::render ──depth + colour──▶ draw_particles (mscent-render)
 (with dynamic lights)                     shards ▶ half-res layer ▶ over ▶ additive ▶ beams
```

- **Simulation** goes in `mscent-game/src/fx/`. The game crate already depends on math, world,
  collide and render, so there is no new crate and no new crate edge. `Game` never reads `Fx`, so
  effects cannot change gameplay or break replays, and a test asserts that.
- **Drawing** goes in `mscent-render/src/particles/`, because the kernels need the `pub(crate)`
  `Framebuffer::row_mut` and belong with `simd.rs`.
- **No data files.** Effects are `const` Rust tables, which adds no untrusted input and no parser.

## Simulation

**Particle classes.** A class holds everything shared, so particles stay small and loops stay tight:

```rust
pub struct ParticleClass {
    pub prim: Prim,            // Spark | Puff { shape, flipbook } | Shard
    pub blend: Blend,          // Add | Over
    pub tier: Tier,            // Essential | Normal | Cosmetic
    pub gravity: f32,          // world units/s^2 along -Z; 0 for most, this is 6DoF
    pub drag: f32,
    pub collide: Collide,      // None | Die | Bounce { keep: f32 }
    pub ramp: &'static [u32],  // premultiplied colour and alpha over life
    pub size: (f32, f32),      // world radius at birth and at death
    pub stretch: f32,          // sparks: tail length per unit of speed
    pub trail: Option<Trail>,  // class and interval, for smoking debris
}
```

**Storage.** One fixed-capacity structure-of-arrays pool per class, allocated at level load, with
death by swap-remove. Position, velocity, age, inverse life and a `key: u8` assigned at birth. A
full pool drops the new particle, except in the Essential tier, which replaces its oldest. Total
capacity is 4096. Nothing allocates after load, and `tests/frame_alloc.rs` gets a sibling that
asserts it.

**Stepping.** Semi-implicit Euler at 60 Hz (every second game step), drawn at
`pos + vel * alpha * dt` with no stored previous position. The particle clock keeps running while
the phase is `Won` or `Lost`, so a fatal explosion finishes instead of freezing, and it stops only
on the pause menu. `Session::step` runs the game stepper only while `playing()`, so `Fx` needs its
own stepper. It has its own seeded `Rng`, never the enemies'.

**Collision** has three levels, chosen by what was measured:

| Class setting | Cost per tick | Used by |
|---|---|---|
| `None` | nothing; the depth test hides what passes through a wall | smoke, fire, flashes |
| `Die` | one `is_solid` | dust, small sparks |
| `Bounce` | one `is_solid`, plus one short sweep on entry for the normal | big sparks, shards |

Sweeps are limited to 32 per tick and `is_solid` to 512. Over the limit, the rest of the tick's
particles skip the test: a spark passes through a wall, where depth hides it. A bouncing particle
that finds no sweep budget dies. Reference cost at the limits is about 90 us per tick.

**Effects** are tables of emitters, spawned as a burst or over time:

```rust
pub struct Emitter {
    pub class: ClassId,
    pub count: (u16, u16),
    pub shape: Shape,          // Point | Sphere(r) | Cone { half_angle } | Hemisphere | Segment | Volume
    pub speed: (f32, f32),
    pub life: (f32, f32),
    pub delay: f32,            // seconds after the trigger
    pub tint: TintSource,      // Class | Surface
}

pub struct EffectArgs {
    pub pos: Vec3,
    pub dir: Vec3,             // surface normal or shot direction; zero means any
    pub scale: f32,
    pub surface: SurfaceTint,  // mean albedo of the struck material
    pub volume: Option<Aabb>,  // the box a Volume shape fills
}
```

`Fx::spawn` rejects non-finite arguments and clamps scale and counts, so it never panics on values
that trace back to a level. Delayed emitters wait in a small fixed queue. Effects at a distance
spawn fewer Normal and Cosmetic particles, and effects beyond fog end spawn none, which matters
because default fog is total at 26 units.

## Drawing

**Order**, after the scene and meshes and before the HUD:

1. **Shards** through the existing `draw_polygon` with `FlatShader`, writing depth. A shard is a
   textured triangle, so brick debris looks like brick and nothing new is rasterized.
2. **Half-res layer**, only when active (see Budget), composited over the frame.
3. **"Over" sprites** back to front: a 32-slice counting sort on `1/z`, reusing preallocated space.
4. **Additive sprites** unsorted, since addition is commutative.
5. **Sparks and beams**.

Particles do not write depth. Fire is drawn over smoke rather than sorted with it, which reads as
fire glowing through smoke and removes one sort.

**Sprite bank.** Masks are `u8` coverage generated from a seed at load: soft disc, four noisy puffs
(rotation variety without rotating pixels), ring, and an 8-frame fireball flipbook, each at sizes 8
to 128, about 300 KB in all. No asset files, no decoder path. A sprite picks the next mip up and
fetches rows through a per-sprite u-table, so size changes smoothly with no popping. There is no
bilinear filter, which suits the soft masks and the nearest upscale the game already does.

**Kernels** live next to `simd.rs` under the same rules: a scalar definition, SSE2 and optional
AVX2 paths, tests that they are bit-identical, a `// SAFETY:` comment on each block.

```
add:  dst = saturate(dst + mask*tint/256)        if 1/z >= depth
over: dst = mask*tint/256 + dst*(256 - mask*a/256)/256
```

Tint and depth are per sprite, so the depth test is one vector compare against a constant and fog
is one colour change per sprite: additive tints fade to black, "over" tints lerp to the fog colour.

**Beams** are the one primitive with a depth that varies along it. A beam is a 3D segment clipped
to the near plane, walked along its major axis with `1/z` stepped linearly (it is affine in screen
space), and a cross-section 1 to 5 pixels wide taken from a small profile table and chosen by
depth. Near-plane clipping matters: a shot arriving at the player passes through the camera.

**Bands.** `draw_particles` takes a built draw list and a `&mut Framebuffer`, clips to
`row_range()` and keeps no state between rows, so it gives identical pixels at any worker count. A
test mirrors `one_and_many_workers_give_identical_pixels`.

### Dynamic light

A flash on the walls is most of what makes an explosion read as filling the room. The design uses
per-face lights rather than a screen-space pass:

- `render_with` already picks a shader per face. A face within reach of a light and facing it gets
  a `LightingShader` variant that adds the light to `col[]` after `plan.colors`, evaluated at both
  span ends and stepped between them. Every other face runs today's code at no extra cost.
- This adds **before** the albedo and fog, so a dark wall lights up like a wall, where a brightening
  pass would only scale what is on screen. It also has a real `N.L` from the face plane, and no
  light bleeding across depth edges, which the measured 221 us screen-space version has.
- On a spawn, a bounded portal flood from the light's leaf marks which leaves it can reach, so a
  blast does not light the room behind a wall. This runs per light, not per frame.
- Enemy meshes get the same lights through `MeshLighting.points`.

Limit: 4 lights at once, each with a short life. The per-span cost is an **estimate** (about 100 to
150 us on the dev machine at full-screen coverage). M9.5 has to measure it, and if it is much worse
the screen-space pass is the fallback.

## Budget

The budget is in **pixels, not time**, so the same script gives the same picture.

Each sprite's cost is its clipped bbox area times a weight taken from the table above: additive 1,
"over" 1.7, streak and beam pixels 5.8 (the scaled fetch is included). Budget at full size:

| Quality | Weighted screens | Lights | Density | Half-res layer |
|---|---|---|---|---|
| High | 5.0 | 4 | 1.0 | yes |
| Medium | 3.0 | 2 | 0.7 | yes |
| Low | 1.5 | 1 | 0.4 | no |

A weighted screen is 230k additive pixels at 640x360, about 92 us on dev. High is therefore about
0.46 ms of CPU on the dev machine and **about 2.3 ms on the reference at the 5x guess**, or 14% of a
frame on one worker. Fill splits across bands, so that wall time falls with workers. The budget
stays in pixels and is not raised for them: the same script must give the same picture at any
worker count. It is a multiple of the internal size, so it shrinks along with the scene when the
resolution drops.

**How a frame is fitted**, in the order the collect pass runs:

1. Project every particle, cull against the frustum, drop those fully fogged, and compute cost.
2. For sprites over 32 px, probe depth at five points and scale cost by the visible fraction. A
   hidden sprite is still drawn, cheaply, so a wrong probe costs a little time and never a visible
   error. This stops an explosion behind a wall from starving the one in front of it.
3. Sum cost per tier. **Essential is always drawn.**
4. If the large "over" sprites alone exceed the remainder and the quality allows it, move sprites
   over 64 px to the half-res layer at a quarter of their cost, charging its fixed overhead first.
5. Compute `keep` for Cosmetic, then Normal. A particle draws while `key < keep`, with opacity
   fading across a band just above the cut. The same particles survive from frame to frame and
   fade rather than pop. `keep` is low-passed so a spike does not flicker.

**Adaptive part, paced runs only.** The host times the particle pass. Over the slice for several
frames, a multiplier on the budget comes down, and it recovers slowly. Headless and scripted runs
fix it at 1, the same way they fix the internal scale.

**The resolution controller does not see the particle pass.** `app.rs` reports `render` cost minus
particle cost. An explosion lasts half a second and the controller takes 30 frames to climb back,
so otherwise each explosion would blur the picture for longer than it lasts. The cost is that a big
explosion can miss a frame deadline when the scene is already at its limit. That is the better of
the two failures, and the budget multiplier limits how long it lasts.

## Effect recipes

Starting values, to be tuned against screenshots. Radii are world units; a drone's radius is 0.75.
Costs are estimates from the measured kernels for the case described, on the dev machine; multiply
by 5 for the reference.

**Laser shot travelling.** Gameplay stays hitscan (see Decisions). A bolt is a beam head moving at
400 units/s with flight time capped at 80 ms, so it covers any range in at most 5 frames at 60 fps.
Tail 4 units, white core with an orange edge for enemies and cyan for the player, an additive glow
sprite on the head, a muzzle puff at the origin, and a short light on the head. The player's bolt
starts from a gun offset to the lower right of the eye. **Under 2 us each.**

**Laser shot hitting various materials.** On arrival, with the normal and the material from
`Scene::material_at`, spawn the shared core plus a material row:

| Material | Sparks | Debris | Lingering | Character |
|---|---|---|---|---|
| `METAL` | 16, bouncing, white to orange | none | glowing spot 0.4 s | bright, ringing |
| `BRICK` | 4, short | 6 chips, surface tint | 4 dust puffs, red-brown | crumbling |
| `ROCK` | 3, short | 8 chips, surface tint | 5 dust puffs, grey | heavier, slower |
| `FLOOR` | 8 | 3 chips | 2 dust puffs | between metal and brick |
| `LIGHTPANEL` | 10, cold white | 6 glass shards | flicker flash | glass breaking |
| unknown id | 8 | none | 2 puffs | safe default; levels are untrusted |

The core is a flash sprite along the normal and a scorch decal (M9.7). Material tints are read once
at level load from each texture's smallest mip. **About 4 us per hit**, about 20 us on the
reference, at 4 shots per second.

**Laser shot hitting a drone.** No surface row. 20 cyan-white sparks in a cone around the reflected
direction, two small additive flashes, 2 shards, and a mesh flash from a 0.1 s light at the hit
point, which lights the drone by itself. **About 3 us.**

**A drone exploding.** Triggered on `EnemyKilled`:

| Delay | Emitter |
|---|---|
| 0 | flash sprite, radius 3, 3 frames; light radius 12, 0.25 s |
| 0 | 6 fireball flipbook sprites, sphere radius 0.7, growing 1 to 2 |
| 0 | 60 sparks, half bouncing |
| 0 | 16 shards with the drone's colours, bouncing, tumbling, 1 in 4 trailing smoke |
| 0.05 s | ring sprite, radius 0.5 to 4, 0.2 s |
| 0.1 s | 12 smoke puffs, lit by the local level lights, 1.5 s, slow drift |

At 5 units from the camera: **about 110 us**, about 0.55 ms on the reference, well inside the budget.

**A large room-clearing explosion.** `Effect::Blast { radius }`. Nothing in the game fires one yet,
and damage from it is a separate gameplay item.

| Delay | Emitter |
|---|---|
| 0 | white overlay fading over 6 frames with the existing `simd::lerp_toward`; camera shake, cosmetic and applied in `view()` |
| 0 | 4 lights at the centre and pushed toward the walls, radius 2 x blast, 0.6 s |
| 0 | 8 large fireball sprites, then a second wave at 0.1 s |
| 0 | ring sprite expanding to the blast radius, 0.35 s |
| 0 | 400 sparks at high speed, 150 of them bouncing |
| 0 | 64 shards from nearby surfaces where available |
| 0.15 to 0.6 s | 30 smoke puffs filling the volume, 4 s |
| 0.5 to 2 s | 60 slow embers |

Unbudgeted, this is about 14 full-screen layers at the peak, about **2.2 ms on the dev machine and
11 ms on the reference**. This is the case the budget exists for. The half-res layer brings that to
about 0.65 ms (3.2 ms on the reference), still over the High budget of 0.46 ms, so `keep` thins the
remaining 30% from the Cosmetic tier. The white overlay hides the first frames, which are the worst
ones. Those are sums from the kernel table, not a run of the effect, so M9.6 reports the real peak.

**Other materials exploding, for a destructible brush.** The same `Shape::Volume` machinery as
above, with no game code needing to know about brushes:

```rust
fx.spawn(Effect::Shatter, EffectArgs { volume: Some(bounds), surface: tint_of(material), .. });
```

Shard and dust counts scale with the box volume and are clamped, so a huge brush cannot exhaust the
pool. Rows for each material match the hit table, scaled up: metal gives bent plates and sparks,
brick and rock give many chips and thick dust, light panels give glass and a light that dies. The
brush entity itself (level format, CSG update, re-lighting) is out of scope. This only ensures the
effect is ready when it arrives.

## Changes outside the new modules

| Where | Change |
|---|---|
| `combat.rs` | `Hitscan` gains `normal`: from the wall trace, or `(point - centre) / radius` for a sphere |
| `enemy.rs` | `fired: bool` becomes `last_shot: Option<Hitscan>` |
| `game.rs` | new `Event::Shot { origin, end, normal, target }` from player and enemies, alongside the existing events |
| `scene.rs` | `Scene::material_at(point, normal)`: the leaf just off the surface, then its faces on that plane; per shot, not per particle |
| `level/mod.rs` | a public way to get the frame's camera, since `feedback.rs` repeats `NEAR` and `FAR` today; dynamic lights passed to `render` |
| `level/shader.rs`, `lighting_shader.rs` | the lit shader variant for faces a light reaches |
| `session.rs` | owns `Fx`; events to effects; bolt arrival timing; shake in `view()`; removes the muzzle rectangle |
| `feedback.rs` | loses tracers and the muzzle flash; keeps the edge marker for an attacker out of view |
| `app.rs` | reports `render` minus particle cost to the resolution controller; feeds the budget multiplier |
| `mscent_math::epsilon` | the surface probe distance and face-match tolerance for `material_at`, and the decal bias |

Editor Instant Play runs `GameSession`, so it gets all of this with no editor change.

## Files

All under the 500-line limit by construction:

```
crates/mscent-render/src/particles/
  mod.rs       draw list types, draw_particles, pass order
  bank.rs      generated masks, mips, flipbook
  sprite.rs    clipping, u-table, row loop
  kernels.rs   scalar definitions and level dispatch; kernels_x86.rs for the unsafe paths
  beam.rs      near clip, streaks, beams
  layer.rs     half-res layer: depth downsample, composite
  budget.rs    cost weights, probes, keep
crates/mscent-game/src/fx/
  mod.rs       Fx: spawn, tick, collect
  pool.rs      SoA pool, swap-remove
  class.rs     ParticleClass and the class table
  emit.rs      shapes, delayed queue, density
  collide.rs   the three collision levels and their limits
  effects.rs   Effect, Emitter and the recipe tables; effects_surface.rs for the material rows
  lights.rs    the dynamic lights of combat: slots, fade, eviction
crates/mscent-render/src/
  dynlight.rs  DynLight: a placed light; face_points.rs the per-face light; scene/reach.rs the flood
```

## Work items

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M9.0 | **Done.** Tiled frames by default: `JobPool`, one flood and clip per frame, passes over bands | M3.6 | M | `tests/tiled_level.rs`, `jobs_tests.rs`, `tiles_tests.rs`, `tests/frame_alloc.rs`: pixels and depth identical at every worker count and to one band on one worker; one worker makes no thread; allocations equal the old renderer; panics reach the caller |
| M9.1 | **Done.** Kernels, sprite bank, sprite and streak drawing, `fx_bench` in the repo | M9.0 | M | SIMD paths bit-identical to scalar; depth test proven by pixels behind and in front of a wall; bench reproduces the table above |
| M9.2 | **Done.** Pools, classes, emitters, 60 Hz tick, collision levels | M9.1 | M | Zero allocation per frame; same seed gives identical particles; hostile arguments never panic; particles never end in solid under `Bounce` |
| M9.3 | **Done.** Bolts and hits: `Event::Shot`, `Hitscan.normal`, beams, `material_at`, surface rows; remove the tracer | M9.2 | M | Enemy shot behind a wall draws no pixels (the old tracer failed this); each material gives its own output; the replay test is unchanged |
| M9.4 | **Done**, without the adaptive multiplier. Budget: costs, probes, tiers, `keep`, Quality rows | M9.2 | M | A frame at 100 full-screen sprites stays within 10% of budget, Essential is untouched, and a scripted run is pixel-identical twice |
| M9.5 | **Done.** Dynamic lights: per-face shader variant, reach flood, mesh lights | M9.3 | M | No cost when no light is active (frame bench unchanged); wall behind a wall stays unlit; per-span cost measured |
| M9.6 | Drone explosion and `Blast`, half-res layer, shake, overlay | M9.4, M9.5 | L | `fx_gallery` contact sheets looked at by a person; peak cost of `Blast` reported at 640x360 |
| M9.7 | Scorch decals: blended polygon raster, ring of 64, depth bias | M9.3 | S | Decal stays on its face from several angles without z-fighting |
| M9.8 | `Effect::Shatter` and material rows | M9.6 | S | Gallery shows each material; a 1000-unit box stays within pool and budget |

**Evidence.** A new `examples/fx_gallery.rs` renders each effect at fixed times over the showcase
into `target/shots/fx/`, because neither `Blast` nor `Shatter` can be triggered in play yet. A
flythrough script that shoots and kills drones covers the rest end to end. Whether it looks good is
a call for a person with a screenshot; tests can only check that pixels land where they should.

## Decisions

- **Gameplay stays hitscan.** Damage lands on the firing step and the bolt is cosmetic. Real
  projectiles would change balance, the replay traces and the enemy AI for what is a visual
  request. The 80 ms flight cap keeps a kill's explosion and the bolt's arrival close enough that
  the mismatch should not show. A kill still removes the mesh at once, so the explosion is spawned
  at the kill rather than on arrival.
- **Fixed effect tables in code**, not an authoring format. A format is a parser, a new untrusted
  input and a validator, with no user yet. Effects can move to level data if an editor tool wants
  them.
- **Half-res layer rather than dithered fill.** Screen-door or checkerboard fill is cheaper, but it
  shimmers, and shimmer has been a repeated complaint in this project's feedback rounds.
- **No soft particles in the first version.** Large sprites will show a hard line where they cut a
  wall. Fading by depth per pixel doubles the "over" kernel's work. Near-wall blasts are the place
  to look for this, and a per-block fade is the next thing to try if it is objectionable.

## Risks

- **Threads on LazyOS are unproven** until M1.3. The one-worker path is the fallback, at the cost of
  the scene being about 2 times slower than on two cores.
- **The reference factor is a guess.** Everything is stated in pixels so it can be retuned by
  changing numbers once M8.5 runs, with no redesign.
- **Half-res halos.** Taking the nearest depth of each 2x2 block will leave a one or two pixel
  fringe where a large sprite meets a nearer edge. Not yet seen in a picture; M9.6 finds out.
- **Sprite setup cost** was below the timer's noise and could matter for hundreds of small sprites.
  M9.1 measures it, and if it does, small ones become plain points.
- **Effect budget versus frame deadline.** Hiding the particle pass from the resolution controller
  accepts a missed frame during a large explosion when the scene is already at its limit.
