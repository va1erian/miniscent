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

## M10 Level scripting

Design in [scripting.md](scripting.md), language choice in
[scripting-language.md](scripting-language.md).

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M10.1 | `tools/check_size.py` in CI, failing above a budget for the release game binary, proposed at 1.6 MB (1.22 MB today) | | S | The tool fails a deliberately oversized build |
| M10.2 | Level format v3: names, markers, zones, mover groups, `static_brushes` | | M | Round trip, bit-flip fuzz, v2 levels still load |
| M10.3 | `Solids`, mover sweep, push and blocked policies, shots and sight | M10.2 | L | Extended `tests/fuzz.rs`; levels with no movers replay bit-exact; `frame_bench` unchanged |
| M10.4 | `mscent-script`: language and VM, with the fixes listed in scripting-language.md, each file under 500 lines | | L | Language tests, `cargo fuzz` target, hostile scripts, size added under 300 KB |
| M10.5 | Game API through one table, events, commands, tasks in `Game::step` | M10.2, M10.3, M10.4 | M | Each use case as a test; replay trace byte-identical |
| M10.6 | `--check`, `--test`, `--trace`, `--script-api`, `--dump-level`, `--pack-level`, hot reload | M10.5 | M | An agent builds the switch-door level from nothing, with no `cargo build`, in a recorded session |
| M10.7 | Mover rendering and lightmaps | M10.3 | M | Pixel tests read a door at both poses; `frame_bench` with movers recorded in `docs/perf/` |
| M10.8 | Props, pickups, use key, factions, HUD messages | M10.5 | M | Keycard and terminal end to end through `--test` |
| M10.9 | Editor: mover command, names, tags, script errors, reload in Play | M10.2, M10.5 | M | `--selftest` steps |
| M10.10 | Editor: script pane with completion and live checking, basic entities, samples, in-app guide | M10.9 | M | Each sample passes its `--test`; `--selftest` types with real key events; every guide example compiles |

## M11 Voxel volumes

Design in [voxels.md](voxels.md); status in [status.md](status.md#m11-voxel-volumes). Each item leaves the game and editor working, and levels
without volumes draw exactly the same pixels.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M11.1 | `mscent-world::voxel`: chunks, copy on write, resolve, dabs, surface nets, LOD, connectivity; `voxel_bench` example | M2.3 | L | Closed-mesh test over chunk borders; `fold` sign agrees with `Bsp::is_solid`; benchmark reports the field and meshing numbers in the design |
| M11.2 | Level format version 4, section 5 | M11.1, M2.6 | M | Round trip; versions 1 to 3 load; bit flips and forged RLE never panic or exceed the allocation cap |
| M11.3 | Volumes in compile: shell brush, no-draw faces, chunks on leaves, node bounds | M11.1 | M | Seeded `Subtract` volume leaves a room looking the same; a volume with only shell faces still renders; a carved wall shows the material of the brush it came from; a tunnel across two touching volumes has no wall |
| M11.4 | Triangle path, flood, governor, projection and shading | M11.3, M5.5 | L | Pinhole test over 1000 poses; `frame_alloc` unchanged; frame budget met in `check_frame_cost.py`; voxel pixel cost at High compared with a brush pixel |
| M11.5 | Light: occluders, cell records, cache, seam blending | M11.4, M5.8 | L | Voxels shadow brushes and the reverse (pixel tests); `crawl` on a voxel shadow edge; stale and forged caches ignored; painting does not re-bake |
| M11.6 | Collision through `Solids`, contact model, fuzz | M11.3, M4.4 | M | Extended fuzz passes; no lip on a brush to voxel slide; replay stays bit-exact |
| M11.7 | Editor: tool, stroke semantics, auto volumes, live path, undo budget, panel | M11.4, M7.6 | L | Selftest strokes, undoes (picture equal) and plays; worst UI tick under 1 ms during a stroke; a stroke starts no CSG job; a level past the file limits cannot be saved as if it were fine |
| M11.8 | Destruction: queue, atomic swap, inherited light | M11.6, M11.5 | M | Replayed destruction is bit-exact; no frame shows a sky pixel; no ray traced in play |
| M11.9 | Generators: stalactites, mushrooms, coral, trees, hive; Tunnel, Fill, Stamp | M11.7 | L | One showcase level per use case holds its budget; same seed gives the same bytes |

## M12 Texture Studio

Design and what was built in [texture-studio.md](texture-studio.md). All done.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M12.1 | `mscent-texgen`: `Rgba16`, wrapped sampling, tables, periodic noise; `texgen_bench` example; entries in both Cargo profiles | | M | Half-shift seam test for every frequency; same bytes twice; golden hash in CI; noise at 128 px timed |
| M12.2 | First operator set and the parameter table | M12.1 | L | Per-operator tests read real pixels; seam test on every operator and on chains; golden hash per operator; invalid parity and hostile parameters are errors, not panics |
| M12.3 | Confirm the page, store and load model against the Werkkzeug source, then pages to DAG, evaluation, refcounts, cache, recipe text format | M12.2 | L | Round trip; bit-flip and truncation fuzz; `store`/`load` cycles and misuse rejected; memory limit enforced before allocating |
| M12.4 | Output pack and headless bake: `mscent-editor --bake-texture <recipe> <dir>` | M12.3 | M | The normal of a test height at 128 px is within 1 level of `to_normal`; draft and full relief agree; PNGs load through `load_texture` |
| M12.5 | Engine: all of "Engine changes" | | L | Adding a material leaves stock ids and existing-level pixels unchanged; forged manifests return errors; a masked highlight differs by pixel test; a new generation reaches the lists and the 3D view; `check_frame_cost.py` and `crawl`/`stability` before and after |
| M12.6 | Workspace shell: list, op page, parameters, preview, undo, worker | M12.3 | L | `--selftest` builds a recipe through messages and reads preview pixels; `tick_costs` `assert_cheap` during a drag, and a clock-free test that the tick never waits for the worker, as `cache_lock_tests.rs` does |
| M12.7 | `--bake-texture` updates the manifest; lit view, Save and Apply, reload into the 3D view | M12.4, M12.5, M12.6 | M | Selftest applies a change and the 3D view's pixels change while the lightmap key does not; lit view matches the game's shader on a synthetic face; the saved material loads in the game; an agent can make a usable material with no window |
| M12.8 | Paint operator and tool | M12.6 | M | Replay at 128 and 512 px, downsampled, is within 2 levels mean; a stroke across the edge passes the seam test; undo restores identical pixels |
| M12.9 | Later operator set, starter templates | M12.3, M12.4 | L | Same tests as M12.2; each template bakes into a contact sheet |
| M12.10 | Stock recipes for all five materials with `out-specular` for `metal` and `floor` (closes M5.7), retire `gen_textures.py`; a compressing PNG encoder if file size needs it | M12.7 | M | Side-by-side contact sheet reviewed by the owner; full test suite passes, with snapshot changes explained; CI checks that committed PNGs equal the recipe output; assets size recorded |
| M12.11 | Game builds materials from recipes at load | M12.10 | M | Decided on measured numbers: binary size against the budget, load time against the 67 ms PNG decode noted in `build.rs` |

## M13 Viewport surfaces

Design in [viewports.md](viewports.md); status in [status.md](status.md#m13-viewport-surfaces). All done. Levels with no viewport face draw the same pixels.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M13.1 | World: `VIEW_FIRST`, `Entity::VistaLayer` as tag 7, format version 5, level text, islands | M2.6 | M | Round trip; hostile files return errors; v1 to v4 load; islands agree with a flood on generated levels |
| M13.2 | Renderer: viewport face list, resolve, second depth plane, single-layer vista of lit faces, camera line, island flood | M13.1 | L | The tests in viewports.md on occlusion, parallax, workers and islands; no-viewport levels identical; `check_frame_cost.py` before and after |
| M13.3 | Bake: coarse luxels by island, lamps split by island, self-lit faces without luxels | M13.1 | M | The lamp and luxel-size tests; old caches still load; the atlas figure recorded |
| M13.4 | Masked textures: alpha plane and mips, second span loop, the four materials | M13.2 | M | Alpha tests; opaque textures use unchanged code and pixels; frame cost compared |
| M13.5 | Layers, `spin`, `pan`, per-layer fog, `Effects` time, stats | M13.2, M13.4 | M | Layer tests; a replay trace unchanged; screenshots stable at time 0 |
| M13.6 | Editor: entity tool and panel, View entries in the material lists, warnings, New Vista, 3D view shows the sky | M13.3, M13.5 | M | `--selftest` builds a vista, paints a wall, plays it, reads a sky pixel, and undoes to the same picture |
| M13.7 | `assets/levels/facing.txt` demo with the Monument recipe, flythrough script | M13.6 | S | Holds budget on the dev machine; `crawl` and `stability` checked |
| M13.8 | Layers at the frame's quality, voxels in layers, scale in level text and the editor, `mountains`, New mountains | M13.7, M11.4 | L | Shadows read from pixels at every scale; a range at scale 16 bakes less than at scale 1; `default_workers` checked |
| M13.9 | Sky materials as recipes, `out-alpha`, Studio preview of a mask; demo mountains in budget | M13.8, M12.11 | M | The sky PNGs are gone; masked recipes load, preview and save; the demo stays under `VISTA_VOXEL_TRIS` across its platform |
| M13.10 | Play lamps carried into layers' rooms through the layer's map, for lamps that shine on a viewport face | M13.8 | S | Twin luxels across a viewport agree; a lamp behind the wall and an infinite layer change nothing |
| M13.11 | The demo's plain as terrain: `mountains` `clear` and `edge`, one range for the ground, relit | M13.10 | S | The road is flat and the rest rises; the demo stays under `VISTA_VOXEL_TRIS` across its platform; looked at on both renderers |
| M13.12 | Sky light: a layer's room's lamps light the play space through its viewport faces, shadowed in both; the demo's hills brought in | M13.11 | M | A play floor and its image agree; rock in the room and walls of the level shade; an infinite layer sheds nothing; no sky keeps the key |
| M13.13 | Terrain layers: grass, scree, cliff and snow recipes; `Layers` by height, slope and jitter; `layer` and `jitter` in level text; New mountains layered; the demo's ground layered | M13.12 | S | Deterministic bytes, hostile layers refused without a panic, the keywords read and rejected; the demo looked at on both renderers and still under `VISTA_VOXEL_TRIS` |

## M14 Hardware renderer

Design in [hardware-renderer.md](hardware-renderer.md).

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M14.1 | Seams: `FrameInput`, `FrameRenderer`, `LevelAssets`, visible set split from clipping. No GPU yet | | M | Render suite byte-identical; `frame_bench` before and after; software-only size unchanged |
| M14.2 | `mscent-gpu`: device, targets, readback, sky, flat faces. **The gate**: parity test, CI lines, `AGENTS.md`, both agents. Size budgets, WGSL front-end trial | M14.1 | M | Parity passes on your GPU and on lavapipe, with the added CI time recorded; a seeded difference fails; budgets recorded |
| M14.3 | Surfaces in the editor's view: textures, mips, lightmap, fog, emissive, TexInfo, bump, specular, movers, actors | M14.2 | L | Those rows pass, at each `Quality`; constants come from Rust |
| M14.4 | **Editor pane on the GPU**: on by default for levels without voxels, thread-owned device, highlight depth, View toggle, fallback with a reason | M14.3 | M | `--selftest` passes both ways; edit-to-picture and 4K frame cost recorded, with the UI thread's paint measured; an owner has used it in a real window |
| M14.5 | Voxels: plain, blended, spots, detail levels, remesh. Removes the pane's voxel exception | M14.4 | L | Rows pass; a sculpt updates the picture; no sky pixel over 1000 poses, as in `fill_tests` |
| M14.6 | Dynamic lights, particles, decals. Play in the pane on the GPU, by readback | M14.4 | L | Rows pass; `ParticleDraws` byte-equal; Play in the selftest |
| M14.7 | Game window, direct route: `gpu_window`, swapchain, `--renderer`, HUD layer, shared particle estimate | M14.6 | M | A real window at 4K with no readback; a forced failure reaches software; any software picture change recorded |
| M14.8 | Failure paths: device loss, hostile corpus, limits | M14.4 | S | Tests above |
| M14.9 | Speed and default: measure, resolution control on a slow GPU, `auto` by default | All | M | Numbers in `docs/perf/`; a person has played it |
| M14.10 | The pane's paint: find why a new frame costs what the window costs, fix it in xui or give the pane a native surface | M14.4 | L | A new 4K frame painted in under 16 ms, in a real window |
| M14.11 | Viewports on the GPU: viewport faces, vista layers, masked textures, removing the pane's viewport exception | M14.5 | L | Rows for vista, parallax and cutout pass; `facing.msl` draws on the GPU |
| M14.12 | The fine finish: 4x multisampling and trilinear, anisotropic material filtering at High, with depth resolved for reading | M14.4 | M | `tests/finish.rs`: the same picture in the mean, nearer a supersampled truth, each half failing when taken out |

## M15 Enemy foundry

Design and what was built in [enemies.md](enemies.md). M15.1, M15.2 and M15.11 are done. From M15.3 on, the GPU may shade models its own way; software has to look good enough on LazyOS ([hardware-renderer.md](hardware-renderer.md), Two renderers).

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M15.1 | `mscent-world::model`: blueprint reader and writer, limits, cost, build with direct solids, local cuts, cells per part | | L | Round trip; bit-flip and truncation fuzz; same bytes give the same mesh; `model_bench` repeats the build table here, including local cuts; binary size against budget |
| M15.2 | `mscent-render::model`: `Quat` pose, frame preparation, fill, model-space shading, levels, budget, table | M15.1 | L | Texture does not swim; `frame_alloc` unchanged; textured cost per model recorded; side-by-side with flat path |
| M15.3 | Light: leaf test, ambient cube, `glow`, instance tint | M15.2 | M | Wall and zero-lamp pixel tests; compared by eye with the baked picture; `check_frame_cost.py` gains a crowd scene |
| M15.4 | Game: `enemies.txt`, kinds, facing, hit spheres, bounding body; level and script use by name | M15.1 | M | Replay bit-exact; fuzz with large radii; old levels draw the same pixels; a hand-written blueprint plays end to end |
| M15.5 | Motion: parts, sway, debris on death | M15.2 | M | Replay unaffected; moving parts at both ends of a swing by pixel test; no per-frame allocation |
| M15.6 | `mscent-foundry`: helpers, genes, sockets, gates, silhouette, archive; `--foundry` headless | M15.1 | L | Same seed gives the same file; a run of 2000 seeds writes a contact sheet; gates reject planted failures |
| M15.7 | First plans: the nine spiked, with two weak plans reworked | M15.6 | L | Owner reviews a sheet of each; no two kept designs of a plan overlap by over 80% |
| M15.8 | Skin sets: plating, chitin, flesh, glass and two factions as recipes (stock `chitin` and `hull` exist since ids 27 and 28: reuse them or pick other names) | M15.3 | M | Directory limits still met; load time and binary size recorded; ids append-only |
| M15.9 | Modules and the other nine plans | M15.7 | L | Eight modules of each tag; a sheet shows a module on three plans of different classes |
| M15.10 | Editor: Foundry workspace, placing by name, Open as level | M15.6, M15.4 | L | `--selftest` rolls, locks a gene and promotes; worst UI tick during a roll held by `tick_costs` |
| M15.11 | Models on the GPU: the same list, drawn through the voxel pipeline in model space; parity cases | M15.2, M14.5 | M | Parity cases pass on the GPU, each failing with its feature taken out; seeded GPU bugs caught; cost recorded |

## M18 Projectors

Design in [projectors.md](projectors.md). Baked spot lights, drawn by both renderers from the lightmap.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M18.1 | World: `Entity::Projector`, format version 6, level text, validation | M2.6 | S | Round trip in binary and text; hostile and bit-flipped files return errors; a version 5 file holding one is refused |
| M18.2 | Baker: `Cone`, `PointLight::cone`, cone test before the shadow ray, bake key | M18.1 | S | Light inside the cone only, a soft rim, a wall still shadows it; keys of levels with no cone unchanged |
| M18.3 | Renderers: setup, fallback lamp lighting; models and lamp maps skip projectors | M18.2 | S | A frame with a projector is lit under it and dark beyond the cone, and a point light of the same strength is not |
| M18.4 | Editor: Projector tool (J), panel fields, cone in the 2D views, selftest | M18.1 | M | Whatever the fields are given, the entity saves and loads; the selftest saves and reloads one |
| M18.5 | Showcase gets four projectors | M18.3 | S | Loads, compiles, entities in empty space |
| M18.6 | Projected shadows: projectors hold the static-lamp maps, the shader applies the cone | M18.3 | M | A mover shades the floor in a projector's pool, and not outside its cone, from real pixels |

## M19 Enemy designer

Design in [enemy-designer.md](enemy-designer.md). Enemies designed in the editor, by generator or by hand, saved in
the level and played as real enemies. Replaces M15.4, M15.10 and the editor half of M15.6.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M19.1 | World: kinds section (format 7), text block, stock table for old levels, limits, level cost | M15.1 | M | Round trip binary and text; bit-flip and truncation fuzz; every limit; old levels read the stock table |
| M19.2 | Game: kinds from the level, build at load, facing, hit spheres, scripts by name; binary size measured | M19.1 | M | Replays bit-exact; a hand-written kind plays end to end through `--test`; size recorded against 2.2 MB |
| M19.3 | Renderers: enemies drawn as their kind's model on both; `Mesh::enemy` deleted; friendly ring | M19.2, M15.11 | M | Frame tests on both find the model's pixels where the ball was; a showcase PNG looked at |
| M19.4 | Editor placing: kind dropdown with thumbnails, model in the 3D pane, name and warning in 2D | M19.1, M19.3 | S | Selftest places two kinds, saves, reloads, and both draw |
| M19.5 | Foundry core in the editor: plans, genes and locks, emit, gates, fit; sentry, dart, mantis plans | M19.1 | L | Same seed same text; 500 seeds per plan read back; gates reject planted failures |
| M19.6 | Enemy Designer workspace: kinds list, sheet, preview, design and text tabs, Apply, library | M19.4, M19.5 | L | `--selftest` rolls, locks a gene, edits text, applies, saves, reloads and plays the kind; `tick_costs` held |
| M19.7 | `--foundry plan --seeds N --out dir` headless contact sheet | M19.5 | S | Writes a sheet PNG and the gate table for 2000 seeds |

## Suggested order

M0, then M1.1 through M1.4 in parallel with M2.1 through M2.3. The renderer
(M3) and collision (M4) both depend only on the BSP and can proceed
side by side. Lighting (M5) follows the first renderer flythrough. The editor
(M7) starts once the game loads levels, and the port (M8) closes the project.
