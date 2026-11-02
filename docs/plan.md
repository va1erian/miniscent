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
- Game elements are the player start, point lights, enemies, a goal to reach,
  and what level scripting adds: movers, zones, markers, and props spawned by a
  script ([scripting.md](scripting.md)).
- Rock-solid collision with the world and with other entities.
- Level editor with instant play, built on xui.
- Windows and LazyOS.

## Reference machine

The target is the Kaby Lake box LazyOS is developed against: an Intel Core
i3-7100U, 2 cores and 4 threads at 2.4 GHz (no turbo), 3 MiB L3, AVX2 in
hardware (`lazyos/docs/compat/kabylake/hw-survey/03-cpuinfo.txt`).

- 60 fps is 16.6 ms, about 40 M cycles per frame.
- LazyOS's kernel enables AVX state (`CR4.OSXSAVE`, `XCR0`) at boot when the
  CPU has `XSAVE` and `AVX` (`lazyos/docs/architecture/arch.md`). QEMU's
  default `qemu64` model has neither, so AVX2 is available on the real box but
  not in the default emulator; M1.3 confirms it on the box.
- Only a `LAZYOS_SMP=1` kernel runs user tasks on every core, in parallel; the
  kernel entry lock serializes only time spent in the kernel. Any other kernel
  keeps the other cores idle.
- The renderer must therefore work with one worker on the SSE2 baseline, and
  use more workers and AVX2 where the platform offers them.

## Decisions

| Area | Choice | Why |
|---|---|---|
| Language | Rust workspace, `no_std + alloc` in core crates where practical | xui and LazyOS are Rust; keeps the LazyOS port cheap |
| Renderer | Software only | LazyOS has a framebuffer and no GPU driver for rendering |
| Platform layer | A small `mscent-platform` trait (present a framebuffer, poll input with relative mouse, time, quit); desktop backend on winit + softbuffer, headless scripted backend, LazyOS backend later. The game does not use xui; only the editor does | Keeps the game small and portable between Windows and LazyOS |
| World | Ordered add/subtract convex brushes compiled to a solid/empty-leaf BSP with portals | The original Unreal model |
| Collision | Swept sphere against the BSP, analytic sphere-sphere for entities | Tunnel-proof; a sphere suits 6DoF |
| Lighting | Offline bake: colour plus a dominant light direction per luxel | Enables bump mapping at no per-pixel lightmap cost |
| Bump mapping | Tangent-space normal map against the baked direction; the light itself is sampled per pixel (was per 16 px span) | Measured in `examples/bump_bench.rs` at 640x360: bump roughly doubles span-stepped shading (0.47 to 0.94 ms per frame on the dev machine, an estimated 4 to 5 ms on the i3-7100U at overdraw 1); evaluating the light per pixel instead of per span costs about twice that again |
| Specular | High-detail option only, from the same baked direction and a per-material gloss | No extra lightmap data; zero cost when off |
| Level scripting | One script per level in a small in-repo Smalltalk-style language, no logic entities | About 228 KB against 4 MB for Rhai, no dependencies, real suspension, typos caught at load ([scripting-language.md](scripting-language.md)) |

## Deviations since the first plan

- **Textures from recipes.** Stock materials ship as recipes that the game runs at load, in the
  manner of .kkrieger, edited in the editor's Texture Studio ([texture-studio.md](texture-studio.md)).
  Renderer depends on `mscent-texgen` for this.
- **TexInfo.** Faces map textures with a Hammer-style per-plane `TexInfo` (axes, shift, scale,
  rotation, flips, fit, justify), not a fixed planar projection; level format version 2 stores it.
- **Stock xui.** The editor builds against upstream xui (rev `0b1b7ef`, which has the monitor-change fix, pointer lock and F5 to F12 keys); nothing is vendored. The game has no xui dependency.
- **Threaded play.** Instant Play runs the game on a render thread with a one-slot frame mailbox,
  not on the UI thread, so a slow scene never slows the UI.
- **Parallel frames by default.** The game renders in bands on a pool of parked threads with at
  least 4 workers (`jobs.rs`), with visibility and clipping done once per frame. A kernel or platform
  without useful threads takes the one-worker path through `LevelRenderer::set_tiling`.
- **Bake cache and soft shadows.** The lightmap is baked once per distinct set of inputs
  (`BakeCache`, optional file mirror) and frames never touch it; lamps are discs so shadow edges are
  smooth, and albedo is mipmapped, to stop shimmer at a distance.

## Architecture

```
crates/
  mscent-math      vectors, planes, AABB, the epsilon policy
  mscent-world     brushes, CSG to BSP, portals, lightmap bake, level format
  mscent-collide   trace and sweep against the BSP and entities
  mscent-render    software rasterizer
  mscent-script    the level scripting language and VM; knows nothing about the game
  mscent-texgen    procedural textures: operators, block graph, recipe files
  mscent-game      6DoF player, enemies, goal, fixed-timestep loop, script API
  mscent-platform  Platform trait, desktop (winit) and headless backends; LazyOS backend later
apps/
  game/            runtime, no xui
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
direction are sampled per pixel with bilinear filtering from a lightmap mip
chain (deviation: the first design evaluated them at 16-pixel span ends, which crawled
with the camera). The baked atlas is persisted next to the level (`.msl.lit`). Per pixel the renderer fetches the texel and its normal, then does
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
hit plane, repeated a bounded number of times. The sweep is a ray against each
convex solid cell expanded by the sphere radius. Face contacts use the planes
pushed out by the radius. Edge and vertex contacts are tested exactly (ray
against the cylinder around each edge and the sphere around each vertex), because
pushed-out planes alone leave square corners and stop the sphere early at convex
edges, an invisible wall. Quake's precomputed fixed-size hulls do not cover an
arbitrary radius. The physics step is fixed (120 Hz) and deterministic. Tests include a property-based fuzzer that fires
random sweeps into random levels and asserts the sphere never ends inside
solid and never skips a wall.

### Editor

An xui app with three orthographic views and a 3D view that uses the same
rasterizer. Tools create and resize brushes, toggle add/subtract, and place the
player start, lights, enemies and the goal. There is no build command: the 3D view follows every
edit, on draft lighting at once and on the full bake when the edits stop, all off the UI thread.
Instant Play starts the game in-process, on its own thread, on the level in the 3D view, and `Esc`
returns to the editor. The tool palette is icon buttons; F5 plays (Ctrl+R as a fallback). Clicking a
face in the 3D view edits its Hammer-style texture mapping.

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
  committed. Findings are fixed or answered in the PR before merge.

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
  frame through the kernel. A full 2560x1440 RGBA frame is 14.7 MB, about
  880 MB/s at 60 fps, and the upscale to the window comes on top. The
  benchmark excludes both, so M1.3 measures them on the box before the
  renderer is designed around an internal resolution.

## Open questions

- Should voxel volumes join the requirements? Design proposed in [voxels.md](voxels.md).
- Should levels see out into a larger world? Viewport surfaces and layered skies are proposed in [viewports.md](viewports.md).
- Is a weapon in scope? Enemies need some way to be handled; work item M6.5
  assumes a single primary-fire weapon and nothing else.
- Does "point" in the game-element list mean point lights, as assumed here?
- Licence and repository host for the project.
