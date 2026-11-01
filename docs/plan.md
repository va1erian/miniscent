# Miniscent: plan

A Descent-style, level-based, six-degrees-of-freedom FPS. Levels are built from
additive and subtractive CSG brushes (the original Unreal's model), lit by a
precomputed lightmap, and drawn by a software renderer that holds 60 fps on a
Kaby Lake machine. It runs on Windows and on
[LazyOS](https://github.com/va1erian/lazyos). The level editor is an
[xui](https://github.com/va1erian/xui) app with instant play. The work items
are in [work-items.md](work-items.md).

## Requirements

- Six degrees of freedom, level based.
- CSG brush worlds, additive and subtractive, ordered.
- Precomputed lightmaps.
- Software renderer, 60 fps with every graphics feature on, on the reference
  machine. No hardware renderer.
- Game elements are limited to: the player start, point lights, enemies and a
  goal to reach.
- Rock-solid collision with the world and with other entities.
- Level editor with instant play, built on xui.
- Windows and LazyOS.

## Reference machine

The target is the Kaby Lake box LazyOS is developed against: an Intel Core
i3-7100U, 2 cores and 4 threads at 2.4 GHz (no turbo), 3 MiB L3, AVX2 in
hardware (`lazyos/docs/compat/kabylake/hw-survey/03-cpuinfo.txt`).

- 60 fps is 16.6 ms, about 40 M cycles per frame.
- On LazyOS the default kernel runs user code on one core with SSE2 only.
  A `LAZYOS_SMP=1` kernel runs user tasks on every core in parallel; the
  kernel entry lock serializes only time spent in the kernel. Whether AVX is
  enabled there is unverified (work item M1.3).
- The renderer must therefore work with one worker on the SSE2 baseline, and
  use more workers and AVX2 where the platform offers them.

## Decisions

| Area | Choice | Why |
|---|---|---|
| Language | Rust workspace, `no_std + alloc` in core crates where practical | xui and LazyOS are Rust; keeps the LazyOS port cheap |
| Renderer | Software only | LazyOS has a framebuffer and no GPU driver for rendering |
| Platform layer | An xui canvas widget presenting a framebuffer | One path on Windows and LazyOS, reused by the editor |
| World | Ordered add/subtract convex brushes compiled to a solid/empty-leaf BSP with portals | The original Unreal model |
| Collision | Swept sphere against the BSP, analytic sphere-sphere for entities | Tunnel-proof; a sphere suits 6DoF |
| Lighting | Offline bake: colour plus a dominant light direction per luxel | Enables bump mapping at no per-pixel lightmap cost |
| Bump mapping | Tangent-space normal map against the baked direction, evaluated per 16 px span | Measured at about 15% cheaper than a per-pixel bilinear lightmap (`examples/bump_bench.rs`) |
| Specular | High-detail option only, from the same baked direction and a per-material gloss | No extra lightmap data; zero cost when off |

## Architecture

```
crates/
  mscent-math      vectors, planes, AABB, the epsilon policy
  mscent-world     brushes, CSG to BSP, portals, lightmap bake, level format
  mscent-collide   trace and sweep against the BSP and entities
  mscent-render    software rasterizer
  mscent-game      6DoF player, enemies, goal, fixed-timestep loop
  mscent-platform  xui framebuffer and input glue (Windows, LazyOS)
apps/
  game/            runtime
  editor/          xui level editor with instant play
```

### CSG and BSP

A brush is a convex set of planes with an add or subtract flag and an order
index. The world starts solid, as in Unreal. Brushes are merged in order into a
BSP whose leaves are solid or empty (a Naylor-style tree merge). Planes are
snapped to a grid and built in `f64`, then stored as `f32`. All tolerances
live in one module. The output is the visible polygons of empty space, the
collision BSP and the portals between empty leaves.

### Lightmaps and bump mapping

Each face gets a planar UV projection and a place in a lightmap atlas. The
baker traces shadow rays through the BSP and stores, per luxel, a light colour
and the dominant light direction in the face's tangent space. Faces are planar,
so the tangent frame is constant per face. At draw time the light colour and
direction are evaluated at the ends of each 16-pixel span and stepped
linearly. Per pixel the renderer fetches the texel and its normal, then does
one dot product. Specular, when enabled, uses the half-vector of that
direction and the view, a small power lookup table and a per-material gloss.

### Renderer

Internal resolution is chosen to hold 60 fps (640x360 as the starting point)
and upscaled to the window, the way LazyOS's `xui-golf` adapts an integer
scale to frame cost. The scene is traversed front to back through the portals
with a z-buffer. It uses perspective-correct texturing with 16-pixel subdivision, distance fog and
a sky. Enemies and the goal are small low-poly meshes lit from the lightmap at
their position. The screen is split into tiles for workers, with SIMD where the
platform allows. A headless benchmark with a frame-time budget guards 60 fps.

### Collision

The player and enemies are spheres. Movement is a sweep, then slide along the
hit plane, repeated a bounded number of times. Brush planes are pushed out by
the sphere radius, as in Quake's `trace`. The physics step is fixed
(120 Hz) and deterministic. Tests include a property-based fuzzer that fires
random sweeps into random levels and asserts the sphere never ends inside
solid and never skips a wall.

### Editor

An xui app with three orthographic views and a 3D view that uses the same
rasterizer. Tools create and resize brushes, toggle add/subtract, and place the
player start, lights, enemies and the goal. Build runs CSG, portals and the
bake. Instant Play starts the game in-process on the freshly built level, and
`Esc` returns to the editor.

## Code standards

Identical to LazyOS (`AGENTS.md`, `CONTRIBUTING.md` and the "Code standards"
section of its `README.md`):

- **Security first.** Treat every level file, and everything else read from
  disk, as untrusted. Validate lengths and ranges, use checked arithmetic, fail
  closed, and never panic on untrusted input.
- **`unsafe` is minimal** and every block carries a `// SAFETY:` comment.
  Clippy runs with `-D warnings -D clippy::undocumented_unsafe_blocks`.
- **Readable.** Small single-purpose functions, descriptive names, types that
  make invalid states unrepresentable, comments that explain why. No dead code
  and no speculative abstraction. Match the surrounding code.
- **Files stay under 500 lines**, enforced in CI for every tracked `.rs` and
  `.py` file. Split by responsibility instead.
- **Pinned toolchain** in `rust-toolchain.toml`, with `clippy` and `rustfmt`.
- **Evidence over assertion.** Graphics are verified by reading real pixels
  (offscreen render snapshots and `pngstats`-style assertions), collision by
  property tests, performance by the frame-time benchmark. A change is not
  claimed to work from source alone.
- **Performance is a feature.** A change that slows the frame or the input path
  is a regression even when correct. Measure on the reference machine.
- **Before pushing:** `cargo fmt --all -- --check`, `cargo clippy --all-targets
  -- -D warnings -D clippy::undocumented_unsafe_blocks`, `cargo test`, the
  file-length check, and the docs check.
- **Pre-PR review is required** after the CI basics pass and the branch is
  committed.

On LazyOS the game also follows its app conventions: launchable from
`run_demo.py` and the GUI launcher with a build switch and a core package, a
trace channel instead of printed markers, and paths from `libs/fhs`.

## Risks

- **Numerical robustness of CSG.** Mitigated by grid snapping, one epsilon
  module and a large test corpus.
- **LazyOS unknowns:** threads in user code, SMP kernel, AVX enablement, relative
  mouse capture in the xui canvas under `xuid`. Settled by the spike M1.3.
  Fallbacks: one worker, SSE2 only, keyboard look.
- **Frame budget.** The benchmark leaves out perspective division, the z-buffer,
  sprites, the upscale and the present. The numbers in `bump_bench` show
  feasibility only; M1.4 measures on the real machine.
- **Present cost on LazyOS.** The display grant copies a damage rectangle per
  frame, and it must fit in the frame budget.

## Open questions

- Is a weapon in scope? Enemies need some way to be handled; work item G5
  assumes a single primary-fire weapon and nothing else.
- Does "point" in the game-element list mean point lights, as assumed here?
- Licence and repository host for the project.
