# Work items

Derived from [plan.md](plan.md). Every item follows the code standards there:
small functions, `// SAFETY:` on every `unsafe`, files under 500 lines, clippy
clean with `-D warnings`, tests that read real output. "Deps" lists the items
that must land first. "Done when" is the evidence a reviewer checks.

Sizes: S is under a day, M a few days, L a week or more.

## M0 Foundations

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M0.1 | Cargo workspace with the crate layout from the plan, a pinned `rust-toolchain.toml` (clippy, rustfmt) and workspace lints | none | S | `cargo build` and `cargo clippy --all-targets -- -D warnings -D clippy::undocumented_unsafe_blocks` pass on empty crates |
| M0.2 | CI: fmt, clippy, tests, file-length check (port of LazyOS's `tools/check_file_length.py`), docs check | M0.1 | S | A PR with a 501-line `.rs` file fails; a clean PR is green |
| M0.3 | `AGENTS.md` and `CONTRIBUTING.md` adapted from LazyOS's, including the pre-PR review requirement | M0.1 | S | Both files exist and the commands in them run |
| M0.4 | `mscent-math`: vectors, matrices, planes, AABB, and the single epsilon module | M0.1 | M | Unit tests for classification against planes at and around epsilon; property tests for plane splitting |

## M1 Platform and measurement

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M1.1 | `mscent-platform`: xui window presenting an RGBA framebuffer on Windows, with a timing readout | M0.1 | M | A spinning textured triangle at a steady rate; a screenshot test reads the pixels |
| M1.2 | Keyboard and relative mouse input through xui on Windows | M1.1 | S | Look and move input reach a test scene; the cursor is captured and released with `Esc` |
| M1.3 | LazyOS feasibility spike: build the M1.1 app for LazyOS, answer threads, the SMP kernel, mouse capture and `std` support, and confirm on the box that AVX is on (the boot line `HW:CPU:fpu xsave_mask=0x7`); measure the present path and the upscale (full-screen RGBA at the box's resolution, ms per frame) | M1.1 | M | A short report in `docs/` with a yes or no and a fallback for each, and the present and upscale cost in ms; an app that boots under `run_demo.py` |
| M1.4 | Run `examples/bump_bench.rs` on the i3-7100U (Windows) and under LazyOS, record results | M1.1 | S | Numbers in `docs/perf/` for variants A, B, S, C and the specular variant D |
| M1.5 | Extend `bump_bench` with variant D (bump plus specular per span) | M0.1 | S | D reports ms per frame next to A, B, S and C; D is compared with C |

## M2 CSG to BSP (`mscent-world`)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M2.1 | Brush type: convex plane set, add or subtract flag, order index, grid snapping, validation of untrusted brushes | M0.4 | M | Rejects non-convex, degenerate and out-of-range brushes without panicking; unit and fuzz tests |
| M2.2 | BSP node and leaf types, solid and empty leaves, point and AABB classification | M0.4 | M | Tests classify points in hand-built trees |
| M2.3 | Brush insertion into the BSP (add and subtract, in order) | M2.1, M2.2 | L | Tests: room, room with door, nested subtracts, order sensitivity, coplanar faces |
| M2.4 | Polygon extraction for empty-space faces, with face merging and T-junction handling | M2.3 | M | Output polygons close and tile each surface; golden tests on the M2.3 cases |
| M2.5 | Portal generation between empty leaves | M2.3 | M | Every empty leaf pair that touches has a portal; flood fill reaches all of a connected level |
| M2.6 | Level file format (versioned, bounds-checked reader) | M2.4, M2.5 | M | Round-trips; truncated and hostile files return errors; a fuzz target |
| M2.7 | Seeded random generator of valid brush levels, for tests and fuzzing | M2.3 | M | The same seed gives the same level; every generated level compiles and its portals connect; used by M4.4 |

## M3 Software renderer (`mscent-render`)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M3.1 | Texture pipeline: build-time PNG decode into packed albedo and normal arrays, material table with gloss | M0.1 | M | `assets/textures` loads; a test reads back texels |
| M3.2 | Camera, projection, clipping to the frustum | M0.4 | M | Tests on clipped polygons |
| M3.3 | Span rasterizer: perspective-correct, 16-pixel subdivision, z-buffer | M3.2 | L | Offscreen snapshot test of a textured quad against a reference image |
| M3.4 | BSP and portal traversal front to back with frustum clipping | M2.5, M3.3 | L | A flythrough renders the M2 test levels with no missing or doubled faces |
| M3.5 | Fog, sky and flat-lit fallback | M3.3 | S | Snapshot tests |
| M3.6 | Tile-parallel rendering with a worker count of 1 as a valid setting; SSE2 baseline and an AVX2 path | M3.3 | L | Identical pixels at 1 and N workers; scaling measured; no syscalls in the frame loop |
| M3.7 | Adaptive internal resolution and the upscale to the window | M3.3, M1.1 | M | Scale drops when the frame cost exceeds budget and recovers |
| M3.8 | Frame-cost benchmark on a fixed reference scene, headless. CI gates a machine-independent figure (the ratio to a calibration loop run in the same process); the absolute 16.6 ms budget is checked on the reference box (M8.5) | M3.4 | M | A PR that makes the reference scene 10% costlier relative to the calibration loop fails CI; repeated runs on the CI runner vary by less than the gate |
| M3.9 | Mesh drawing for enemies and the goal, vertex-lit | M3.3 | M | Snapshot tests of a mesh from several angles |
| M3.10 | 2D drawing: bitmap font, clipped rectangles and sprite blit into the framebuffer | M3.3 | M | Snapshot tests of text at several sizes and of clipping at the screen edge |

## M4 Collision (`mscent-collide`)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M4.1 | Sweep a sphere against the BSP's solid cells: face contacts with planes pushed out by the radius | M2.3 | L | Unit tests on walls and slopes, including grazing and glancing hits |
| M4.2 | Slide response with a bounded iteration count and crease handling | M4.1, M4.6 | M | Tests in two-plane and three-plane corners; no jitter |
| M4.3 | Sphere against sphere sweeps for entities | M0.4 | S | Analytic tests, including grazing and at rest |
| M4.4 | Property-based fuzzer: random levels, random sweeps | M4.2, M2.7 | M | The sphere is never inside solid and never passes through a wall, over a large run in CI |
| M4.5 | Fixed 120 Hz physics step with interpolation for rendering | M4.2 | S | Replaying inputs reproduces the same positions |
| M4.6 | Exact edge and vertex contacts: ray against the cylinder around each edge and the sphere around each vertex | M4.1 | M | A sphere swept at a convex edge and a convex corner stops at the true rounded distance, not the square-corner distance; it rounds the corner instead of catching on it |

## M5 Lighting and materials

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M5.1 | Planar UV projection per face and lightmap atlas packing | M2.4 | M | No overlaps, no bleeding; a debug dump of the atlas |
| M5.2 | Baker: point lights with falloff, shadow rays through the BSP, multi-threaded | M5.1 | L | A known scene bakes to the expected values; shadows match a hand-computed case |
| M5.3 | Dominant light direction per luxel in tangent space, stored with the colour | M5.2 | M | Direction is unit length, stable at light-free luxels |
| M5.4 | Lightmap sampling in the rasterizer, evaluated per 16-pixel span | M3.3, M5.3 | M | Snapshot tests; frame cost measured against the bench |
| M5.5 | Bump mapping: normal fetch and one dot product per pixel | M5.4, M3.1 | M | Snapshot against a reference; flat and bumped renders compared |
| M5.6 | Specular as a High option: half-vector, power table, per-material gloss | M5.5 | M | Off by default and free when off; D variant cost recorded |
| M5.7 | Gloss maps in the texture generator (`_s.png`) | M3.1 | S | Generator emits them for `metal` and `floor` |
| M5.8 | Ambient and a cheap bounce pass | M5.2 | M | Rooms lit only through doors are not black |

## M6 Gameplay (`mscent-game`)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M6.1 | 6DoF player: thrust on three axes, pitch, yaw and roll, inertia | M4.5 | M | Input traces reproduce motion; no gimbal artifacts at 90 degrees of pitch |
| M6.2 | Entity model: player start, point lights, enemies, goal, loaded from the level file | M2.6 | S | A level with each entity loads and is placed |
| M6.3 | Enemy behaviour: patrol, chase, attack, with line-of-sight through the BSP | M4.2, M6.2 | L | Scripted scenarios: sees, loses, and re-acquires the player |
| M6.4 | Goal and level flow: win on reaching the goal, lose on death, restart | M6.2 | S | End-to-end test on a small level |
| M6.5 | Primary-fire weapon and damage (assumed in scope; see the plan's open questions) | M6.3 | M | Hits resolve through the same traces as movement |
| M6.6 | HUD and menus | M3.5, M3.10 | M | Snapshot tests |

## M7 Editor (`apps/editor`)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M7.1 | xui editor shell: menus, tool palette, panels | M1.1 | M | The window opens on both platforms |
| M7.2 | Orthographic views with grid, pan and zoom | M7.1 | M | Snapshot tests |
| M7.3 | 3D view using the game rasterizer | M7.1, M3.4 | M | Same pixels as the game for the same camera |
| M7.4 | Brush tools: create, move, resize, flip add and subtract | M7.2, M2.1 | L | Undo and redo cover every tool |
| M7.5 | Entity placement: player start, lights, enemies, goal | M7.2, M6.2 | S | Placed entities appear in all views and survive save |
| M7.6 | Live preview: CSG, portals and bake follow every edit in the background, with errors shown | M2.6, M5.2 | M | Errors are shown, not panics |
| M7.7 | Save and load of level source (brushes and entities) | M7.4 | M | Round-trip tests; hostile files return errors |
| M7.8 | Instant Play: run the game in-process on the freshly built level, `Esc` returns | M7.6, M6.4 | M | Edit, play, return, edit again without leaving the app |

## M8 LazyOS port and release

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M8.1 | LazyOS platform backend from the M1.3 findings, including a single-worker fallback | M1.3, M3.6 | M | The game runs under `run_demo.py` |
| M8.2 | Package and launch: build switch, core package, entries in `run_demo.py` and the GUI launcher | M8.1 | M | Launchable from both front ends |
| M8.3 | Trace channel markers (`UP`, `FPS`, `PASS`, `FAIL`) in place of printed markers | M8.1 | S | The harness reads them |
| M8.4 | Screenshot session harness: boot, play a scripted flythrough, judge the pixels | M8.2, M8.3 | M | A judge self-test and a CI-able run |
| M8.5 | Frame-time results on the real Kaby Lake box, fix the worst offender | M8.1, M3.8 | M | 60 fps with every feature on, written up under `docs/perf/` |
| M8.6 | Windows packaging and release notes | M8.5, M7.8 | S | A zip that runs on a clean machine |

## Suggested order

M0, then M1.1 through M1.4 in parallel with M2.1 through M2.3. The renderer
(M3) and collision (M4) both depend only on the BSP and can proceed
side by side. Lighting (M5) follows the first renderer flythrough. The editor
(M7) starts once the game loads levels, and the port (M8) closes the project.
