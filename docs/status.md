# Implementation status

## How to run

```
# play (desktop window, winit + softbuffer; WASD, mouse, Space fire, Q/E roll, Esc menu)
cargo run --release -p miniscent-game
cargo run --release -p miniscent-game -- --corridor --quality medium --scale 2

# headless screenshots and timing (no window; scripted input, see apps/game/scripts/flythrough.txt)
cargo run --release -p miniscent-game -- --screenshot target/shots/play.png \
    --script apps/game/scripts/flythrough.txt --shots-at 2,12,24
cargo run --release -p miniscent-game -- --bench --frames 200

# level editor (the only xui user); XUI_SNAPSHOT=<dir> renders it headlessly
cargo run --release -p mscent-editor
cargo run --release -p mscent-editor -- --showcase
# the same session and the script workflow: load, edit, save, reload, four views, play, Esc, edit
# again; open a sample, type with completion, see a mistake, play it, add a door, read the guide
cargo run --release -p mscent-editor -- --selftest target/shots/editor

# gate
cargo test --workspace
```

The game never depends on xui. It talks to the small `mscent-platform` trait (present an RGBA
`u32` framebuffer, poll input events including relative mouse, monotonic time, quit). Backends:
a desktop one (winit + softbuffer, cursor grab, cargo feature `desktop`) and a headless scripted
one for tests and screenshots. A LazyOS backend would implement the same trait. xui is used only
by `apps/editor`, which embeds the game in-process for Instant Play through the same trait.

## Summary of this pass (feedback round 1)

`cargo test --workspace` passes (collide 45+3, editor 73, game 41, game app 10+4+5+3, math 40,
platform 12, render 145 plus the integration tests `bake_cache` 5, `forged_cache` 1, `frame_alloc`
1, `hostile_level` 4, `level_render` 6, `review_stress` 2, `shadow_contrast` 1, `snapshot` 4,
`stability` 3, `texinfo_bump_frame` 1, `texinfo_render` 4, `tiled_scene` 1, world 125+1). The
editor selftest (`--selftest target/shots/editor-final`) passes in a dark window and a wide
one. Reported by the user from a real Windows session, and what was done:

- **Monitor move crash (RefCell already borrowed).** Root cause was in `xui-canvas`
  (`ScaleFactorChanged` and `cursor_moved` held a `RefCell` guard across a `borrow_mut()`). Fixed
  upstream (xui#313); the editor now builds against stock xui. Not verified: a real window moved between monitors of different DPI (cannot be done here).
- **Pointer lock.** xui#314 added `Ui::set_pointer_lock` and `Event::PointerDelta`. The editor's 3D
  pane (`ui/view3d.rs`) locks the pointer while the right button is held (fly-look) and Play keeps it
  locked (`View3d::set_grab`, from `begin_play`/`end_play`); `PointerDelta` becomes the look motion.
  A backend that cannot lock never sends deltas, so the old position-difference look and the arrow
  keys remain as the fallback. The lock is released on button release, capture or focus loss, leaving
  Play, and by the backend on Esc, focus loss and node removal; the backend swallows Esc while
  locked, so a `PointerLockLost` in Play ends Play (alt-tab ends it too). Verified headlessly by
  `ui/app/lock_tests.rs` (real editor on the offscreen backend, which records the lock: lock on
  right-down and on F5, camera yaw/pitch change from injected deltas, released on button-up, Esc and
  lock loss). Not verified: a real mouse and cursor on screen (the OS grab path in `xui-canvas`).
- **Live preview, no build command.** F7, Ctrl+B and the Build menu item are gone: every edit
  reaches the 3D pane by itself (`studio/preview.rs`, design in `docs/lightmaps.md`). A new scene
  replaces the old one on the running render thread, so the fly camera and held keys survive an
  edit. Measured in release on the showcase (26 brushes): CSG, faces and portals 6 ms, draft bake
  1.4 ms, full bake 166 ms, material decode 67 ms once per session. Edit to picture 15 ms
  (`studio/preview_tests.rs`) and 29 to 60 ms through the whole UI (selftest), full light 285 to 490
  ms after the edit, worst UI tick in that span 113 microseconds. On a generated 348-brush level the
  first picture takes 706 ms with a worst UI tick of 207 microseconds. Cancelling a 928 ms bake
  ended it in 1 ms. Not verified: a real window, and a level whose full bake takes many seconds
  with real editing during it.
- **Large levels bake in full.** The 348-brush generated level no longer overflows the atlas: the
  height limit went from 4096 to 8192 rows and a level past that coarsens its luxels automatically
  instead of failing (`lightmap/layout.rs`, reasoning and table in `docs/lightmaps.md`). That level
  stays at 0.125 units per luxel: 1024 x 6472, 152 MiB, 19.8 s cold on 24 threads, 250 ms from its
  file, 9.8 ms per frame. In the editor it reaches full light 20.5 s after the first picture (717
  ms) with a worst UI tick of 6 ms. The showcase is bit for bit unchanged. The editor test for this
  runs only under `cargo test --release`: unoptimised, the bake takes minutes even with hard
  shadows, so the debug run covers the layout and a lamp-free bake of that atlas with its file round
  trip. Not verified: bake time on a machine with few cores (it scales with threads, and the 19.8 s
  are on 24), and the coarsened path at the shipped limit end to end: the generator stops at 8 by 8,
  so that path is tested with the limit brought down to 4096.
- **F5 and F7.** Root cause: xui's winit keymap only produced `Key::F1` to `F4`, so a real window
  never raised F5 to F12 (offscreen tests inject events and hid it).
  Fixed upstream (xui#316, #317); the editor builds against stock xui `0b1b7ef` and the vendored
  copy is gone. The
  editor also binds Ctrl+R (play) as a fallback. `selftest/keys.rs` injects
  `KeyDown` F5 and Esc events through the UI and checks the play and the return;
  `ui/app/keys_tests.rs` does the same. Not verified: a physical keyboard.
- **Icon toolbar.** The tool palette is square icon-only toggle buttons with tooltips
  (`ui/toolbar.rs`, `ui/icons.rs`: xui's Lucide icons plus a drawn person, bulb and flag); the
  selected tool is filled with the accent colour. The properties and log columns were
  re-laid out to fit at wide windows (`ui/app/layout_tests.rs`, selftest `wide-*.png`).
- **Lightmap shimmer and faint shadows.** Causes: one-luxel hard shadow edges that alias when seen
  from a distance, albedo sampled without a mip chain, and low shadow contrast. Changes: lamps are
  small discs in the bake (`lightmap/soft.rs`: 4 probe rays, more only in the penumbra) so shadow
  edges are smooth ramps; the albedo uses a box-filtered mip chain (`mscent-render/src/mips.rs`);
  `tests/shadow_contrast.rs` checks the pillar shadow is clearly darker than the lit floor.
  Measured with `cargo run --release -p mscent-render --example stability` (camera moves 1/8 px per
  frame, second difference of luminance on surface interiors): the flat-texture mean was 0.64 to
  0.69 levels before and is 0.04 to 0.09 now (mid and far, strafe and turn); samples jumping by more
  than 3 levels went from 0.48% to 0.09 to 0.24%. With real textures, bump and specular on (High,
  far): mean 0.37 to 0.50 and 0.8 to 1.3% over 3 levels, which includes real texture detail and is
  the remaining shimmer. `tests/stability.rs` asserts the flat numbers.
- **Lightmap cache.** The bake is keyed by its inputs (faces, lights, bake settings;
  `lightmap/store.rs` `input_key`). `BakeCache` (`mscent-render/src/level/cache.rs`) returns the same
  `Arc<Lightmap>` for an unchanged level, can mirror it to a file next to the level, and ignores a
  missing, stale, damaged or forged file (`tests/bake_cache.rs`, `forged_cache.rs`,
  `store_tests.rs`). Frames never bake or copy the atlas: `tests/frame_alloc.rs` measures 256
  allocations and 32 KB per frame in both Low and High, against a 4.77 MB atlas that is untouched.
  In the editor a texture-mapping edit on the showcase reuses the lightmap and never shows a draft
  (`studio/preview_tests.rs`).
  Persistence (feedback round 2): both apps now use the on-disk cache (`<level>.msl.lit`, or the
  per-user cache dir for built-in/unsaved levels), written atomically, with the reason logged when
  a file is ignored. Showcase release: cold bake 49 ms, warm load 8 ms (`tests/lightcache.rs`,
  `bake_cache.rs`). `miniscent --bake|--rebake|--no-cache`; see `docs/lightmaps.md`.
- **Threaded Instant Play.** Play runs on a render thread (`studio/worker.rs`): input goes in over a
  bounded `try_send` channel, frames come out through a one-slot mailbox read with `try_lock`, stale
  frames are dropped, and a slow scene lowers the adaptive resolution instead of stalling the UI. A
  panic in the scene is caught and reported. `studio/tests.rs`: 200 UI ticks run while the test holds
  a frame open, so a tick that waited for it is caught with no clock (release: p90 1 microsecond,
  worst 2.5); uncollected frames are replaced by the newest. `studio/cache_lock_tests.rs` does the
  same for preview and bake jobs by holding the cache lock. Tick cost is asserted at the 90th
  percentile (`studio/tick_costs.rs`), as a single worst case measures the scheduler.
  `studio/race_tests.rs` covers start and stop races. Not verified: responsiveness of a real window
  under load.
- **Hammer-style UV controls.** See the next section.

Not verified: any real window, keyboard or multi-monitor use, the AVX2 paths on the i3-7100U, any
LazyOS run, and `tools/check_frame_cost.py` was not rerun.

## Shadow stability and persistent bake (feedback round 3)

The user reported shadows that still crawl and jag the way shadow maps do, and that they look
generated at runtime. Findings and changes:

- **Cause of the crawl.** The old shader sampled the lightmap bilinearly only at the two ends of each
  16-pixel span and stepped colour and light direction linearly between them, so a pixel's light
  depended on where the screen-aligned span ends fell. It also had no minification filter. The light
  is now sampled per pixel with bilinear filtering (`light_sampler.rs`, `lighting_shader.rs`) from a
  lightmap mip chain (`lightmap_mips.rs`), so a world point gets the same value from any camera.
- **Why the old metric passed.** `examples/stability.rs` measures second differences of luminance on
  surface interiors, which averages over mostly flat areas. The new `examples/crawl.rs` follows fixed
  world points on a shadow edge (luminance noise, contour crawl and stair-step of the contour, at
  full, 750 and 500 permille internal size, 1/16 and 1/4 px camera steps).
- **Numbers now (release, this machine).** Flat Low, floor shadow edge, room_mid: luminance noise
  0.10 to 0.23 levels, contour crawl 0.005 to 0.010 px, stair-step 0.03 to 0.06 px rms (peak 0.14 to
  0.25 px). room_near is the worst: stair-step 0.10 to 0.20 px rms (peak up to 0.86 px). Real High
  (textures, bump, specular): edge luminance noise 0.14 to 2.46 levels (worst: room_far at 500
  permille and 1/4 px steps), contour crawl 0.01 to 0.10 px. `examples/stability.rs` interior shimmer:
  Flat High 0.03 to 0.05 mean, Real High 0.14 (near) to 0.49 (far, strafe); over 3 levels 0.01 to
  1.24%. I did not keep a before-run of `crawl` (it did not exist), so there is no before/after
  figure for the contour metrics; the before is the argument above.
- **Persistent bake.** Both apps use the on-disk cache; first run bakes, later runs load
  (showcase release: cold 49 ms, warm 8 ms). See `docs/lightmaps.md`. The bake itself was always
  offline-style, once per input set; what the user saw as "generated at runtime" was the per-launch
  re-bake, which is gone.
- **Upscale.** Unchanged: `upscale_nearest`. The blur in the screenshot is the adaptive internal
  resolution; no smoother upscale was added, so that part of the report is not addressed.
- **Not verified:** the user's scene on a real machine; whether remaining stair-steps at room_near are
  visible at the real window size; the AVX2 paths on the i3-7100U.
- `cargo test --workspace` passes (one run saw `bake_cache::concurrent_writers_in_one_process_leave_a_valid_file`
  fail under full-workspace load; it passed alone and on rerun, so it is timing sensitive).

## Combat particles (M9.1 to M9.8, `docs/particles.md`)

Shots are world-space now: a bolt with a muzzle flash, an impact chosen by the struck material
(`Scene::surface_at`), sparks on a drone, and a drone explosion on a kill. They are depth tested, so
a shot behind a wall draws nothing (`apps/game/tests/combat_fx.rs`), and the screen-space tracer and
the HUD muzzle rectangle are gone. `Game` emits `Event::Shot` and never reads the effects. A pixel
budget per Quality thins Cosmetic and then Normal particles and never Essential ones, so fill is
bounded and a script gives the same picture on any machine.

A shot that hits a wall leaves a scorch mark when the bolt arrives (M9.7): a ring of 64 in
`fx/decal.rs`, cut to the struck face, darkened by material (`recipes::surface_mark`), drawn
multiplicatively before the sprites with a depth bias in `epsilon::DECAL_DEPTH_BIAS`, faded by fog
and costed as Essential in the budget (about 4 ns per pixel on the dev machine). It stays on its
face from every view tested and flickers in none (`apps/game/tests/decals.rs`). No hot glow on
metal. Seen once in a screenshot, on brick at a glancing angle. Recipe values are starting points
that no player has judged.

Explosions and hits cast a short-lived
point light (at most 4, 2 and 1 at High, Medium and Low) on the walls and drones they reach but not
through walls; the renderer takes them as a slice (`render_with_effects`) and with none the frame is
byte-identical to before. Full-screen cost is 0.2 to 0.3 ms for one light and up to about 1 ms for
four, more than the 100 to 150 us estimated.

Big sprites go through a half-res layer when a frame is over budget (M9.6), which keeps about three
times the sprites for the same cost. `CombatFx::blast` and `shatter` exist with a view kick and
`fx_gallery` contact sheets (`target/shots/fx/`), but nothing in the game fires them, and only the
author has looked at the sheets: fire sprites cut the floor along a straight line, smoke and shatter
are faint. Not there yet: the budget's adaptive multiplier, hiding particle cost from the resolution
controller, soft particles, polygon shards. Effects do not yet respect M10's closed doors: particles,
dynamic lights and a blast's view kick all pass through them.

## Parallel frames by default (M9.0, the groundwork of `docs/particles.md`)

`LevelRenderer::render` draws in 24-row bands with `available_parallelism().max(4)` workers on a
pool of parked threads (`jobs.rs`). Visibility, the BSP walk and the clipping and projection of every
polygon run once per frame (`PolyList` in `raster.rs`); a band fills only the polygons in its rows.
`LevelRenderer::set_tiling` overrides the band height and worker count. Game bench on the dev
machine: 9.29 to 1.20 ms per frame. Pixels and depth equal one band on one worker at every tiling
tried (`tests/tiled_level.rs`), and the scripted flythrough PNGs are byte-identical to the
single-threaded renderer's. Allocations are unchanged (256 per frame, 1379 with actors;
`tests/frame_alloc.rs`). Numbers and the scaling table are in `docs/particles.md`.

Not verified: a real window, the i3-7100U (its 2 cores and 2 hyperthreads), and any LazyOS thread;
no LazyOS backend passes the one-worker override yet. `frame_bench` still draws through `Scene::render`
per band, on one worker, so the CI ratio does not measure this path.

Seen failing on the code before this change as well as after it, and not addressed: the editor
selftest check "a click on the picture picked the framed wall"; `tools/check_frame_cost.py` on the
dev machine (ratio about 8.5 against a 7.61 baseline); intermittently
`a_100_ms_scene_does_not_slow_the_ui_ticks_and_the_newest_frame_is_shown` (a 5 ms limit, under load)
and `bake_cache::writes_are_atomic_and_rebake_overwrites`.

## Per-face texture mapping (TexInfo)

Each brush plane has a Hammer-style `TexInfo` (`crates/mscent-world/src/texinfo.rs`, `texinfo/align.rs`):
axes, shift, scale (world units per texel, default 0.0625 = the old 8-unit tile for 128 px textures),
rotation, flips, fit, justify, align to face/world and copy across an edge. Faces keep it through
CSG and merging, the renderer derives UVs from it, level format version 2 stores it (version 1
still loads with defaults), and the editor document passes it through load and save.

Editor UI (`ui/facepanel.rs`, `ui/faces.rs`, `model/facetex.rs`, `model/pick.rs`): click a face in
the 3D view to pick it (ctrl-click extends the set); the Face / texture section has a material
browser and Face Edit fields applied to every picked face as one undoable command. Picked faces
are highlighted in the 3D view (`studio/highlight.rs`). Selftest steps `08-face-picked.png` to
`11-ctrl-click.png` pick a wall, edit its mapping (the 3D picture changes, lightmap kept), undo (the
picture equals the original) and ctrl-click a second face. Known limit: a face takes its TexInfo
from the plane that owns its centroid, so a re-texturing slab that covers only part of a coplanar
face does not split it. Evidence: `crates/mscent-render/tests/texinfo_render.rs`,
`texinfo_bump_frame.rs`, `crates/mscent-world/tests/texinfo_fuzz.rs`, `model/facetex_tests.rs`, and
PNGs in `target/shots/texinfo/` and `target/shots/editor-final/`.

Brush tools (`model/shape_ops.rs`, `model/tools.rs`, Hammer-style): Slice (C: drag a line in a 2D
view, K picks the kept side, Enter cuts), Carve (Ctrl+Shift+C: the selected brush is removed from
every brush it overlaps, leaving convex pieces), Vertex (V: drag corners; the brush becomes the
convex hull of its corners and a move that loses a corner is refused) and face extrusion (Ctrl+E
out, Ctrl+Shift+E in, by one grid step, on the faces picked with the Face tool; out adds a prism
brush, in moves the face). Each is one undo step. Evidence: `model/tools_tests.rs`. Not run: the
GUI itself was not driven by hand, only the model through the pointer API and the key-table test.

## M0 Foundations

| ID | Status | Evidence |
|---|---|---|
| M0.1 | Done | `Cargo.toml` (workspace lints, `unsafe_code = "forbid"`), `rust-toolchain.toml` |
| M0.2 | Done | `.github/workflows/ci.yml` runs fmt, clippy, tests, `tools/check_file_length.py`, `check_docs.py`, `check_frame_cost.py`; length and docs checks pass locally. A deliberate 501-line PR was not exercised |
| M0.3 | Done | `AGENTS.md`, `CONTRIBUTING.md` |
| M0.4 | Done | `crates/mscent-math` (40 tests; epsilon, plane, polygon, aabb, in-repo seeded `rng.rs` for property tests) |

## M1 Platform and measurement

| ID | Status | Evidence |
|---|---|---|
| M1.1 | Done | `crates/mscent-platform`: `Platform` trait, `desktop.rs` (winit + softbuffer behind feature `desktop`), `headless.rs`, `scale.rs`. Deviation: xui is not the platform (only the editor uses it). The headless backend is covered by tests; a timing readout is in the game `--bench` and the HUD. The desktop window started and quit cleanly with `--autoclose-ms`; no human watched it |
| M1.2 | Partial | `desktop.rs` maps keys and buttons, uses `DeviceEvent` for relative mouse and `set_cursor_grab` (Locked, falling back to Confined); `script.rs` drives the same events headlessly (12 platform tests, game `playthrough` tests). Real cursor capture and release with `Esc` was not exercised by hand |
| M1.3 | Not done | Needs the LazyOS source tree and the physical box. The `mscent-platform` trait is the seam for it |
| M1.4 | Not done | Needs the i3-7100U and LazyOS; `bump_bench` builds and runs on the dev machine, but no numbers are recorded in `docs/perf/` |
| M1.5 | Done | `crates/mscent-bench/examples/bump_bench.rs` prints variants A, B, S, C, D with ratios |

## M2 CSG to BSP (`mscent-world`)

| ID | Status | Evidence |
|---|---|---|
| M2.1 | Done | `brush.rs`; `brush_tests::fuzz_random_planes_never_panic_and_accepted_brushes_are_sane` |
| M2.2 | Done | `bsp.rs`, `bsp_tests.rs` |
| M2.3 | Done | `csg.rs`, `csg_tests.rs`, `polytope.rs` |
| M2.4 | Done | `faces/` (merge, tjunction, cells); `faces_tests::room_with_door_tiles_each_surface`, `t_junctions_are_split` |
| M2.5 | Done | `portals.rs`; `portals_tests::two_rooms_and_a_door_connect_through_portals`, `separate_rooms_are_not_connected` |
| M2.6 | Partial | `format.rs` round-trips and `format_tests::bit_flip_fuzzing_never_panics`; the "fuzz target" is an in-process test, not a `cargo fuzz` target |
| M2.7 | Done | `gen.rs`, `gen_tests.rs`; used by `mscent-collide/tests/fuzz.rs` |

## M3 Software renderer (`mscent-render`)

| ID | Status | Evidence |
|---|---|---|
| M3.1 | Partial | `texture.rs` (`load_texture`) and `material.rs` (`MaterialTable`, gloss, ids from `materials.txt`) with `png.rs` and `inflate.rs`; decoding happens at load time, not build time. `assets/textures` has albedo, `_n` and `_h` |
| M3.2 | Done | `camera.rs`, `clip.rs` and their tests |
| M3.3 | Done | `raster.rs`; `tests/snapshot.rs::brick_quad_facing_the_camera` against `tests/ref/brick_quad.ppm` |
| M3.4 | Done | `scene.rs` and `scene/walk.rs`; `scene_tests::flythrough_matches_brute_force_with_no_doubled_faces` |
| M3.5 | Done | `snapshot::brick_wall_in_perspective_with_fog`, `flat_lit_fallback_scales_brightness`, `framebuffer_tests::sky_gradient_runs_from_top_to_bottom_colour`; the level renderer draws sky, fog and lit surfaces in the flythrough PNGs |
| M3.6 | Partial | `tiles.rs`, `jobs.rs`, `simd.rs`, `simd_x86.rs`; `tiles_tests::one_and_many_workers_give_identical_pixels`, `tests/tiled_level.rs`. Default in the game since M9.0; scaling measured on the dev machine only (`docs/particles.md`), AVX2 path unmeasured, no-syscall property not asserted |
| M3.7 | Done | `resolution.rs` controller plus `upscale_nearest` (`resolution_tests`), wired in `apps/game/src/app.rs` (adaptive path, `--no-adaptive` to disable; headless runs fix the scale). Scale stayed at 1000 in the bench, so a drop under real load was not seen in the game itself |
| M3.8 | Done | `crates/mscent-render/examples/frame_bench.rs`, `tools/check_frame_cost.py`, `docs/perf/baseline.json`, run in CI. The 10% regression failure was not demonstrated |
| M3.9 | Done | `mesh.rs`; `mesh_tests` (fog, lighting, poses) |
| M3.10 | Done | `font.rs`, `draw2d.rs`; `snapshot::text_rects_and_clipping_at_the_screen_edge` |

## M4 Collision (`mscent-collide`)

| ID | Status | Evidence |
|---|---|---|
| M4.1 | Done | `sweep.rs`, `cell.rs`; `sweep_tests::wall_stops_sphere_one_radius_away`, `grazing_parallel_to_wall_is_not_blocked` |
| M4.2 | Done | `slide.rs`; `slide_tests::two_plane_corner_stops_without_creeping`, `three_plane_corner_pins_the_sphere` |
| M4.3 | Done | `spheres.rs` (head-on, grazing, at-rest tests) |
| M4.4 | Done | `tests/fuzz.rs::sweeps_never_enter_or_cross_solid`, `slides_never_end_inside_solid` |
| M4.5 | Done | `step.rs`; `step_tests::replayed_inputs_reproduce_positions_bit_exactly` |
| M4.6 | Done | `prim.rs`; `sweep_tests::convex_edge_stops_at_rounded_distance_not_square`, `sphere_past_the_edge_rounds_it_without_catching` |

## M5 Lighting and materials

| ID | Status | Evidence |
|---|---|---|
| M5.1 | Done | `lightmap/uv.rs`, `atlas.rs`; `lightmap/tests::atlas_has_no_overlap_and_keeps_gutters_apart`, PPM dump |
| M5.2 | Done | `lightmap/bake.rs`, `ray.rs`, `parallel.rs`; `unshadowed_floor_matches_hand_value`, `plate_casts_the_hand_computed_shadow` |
| M5.3 | Done | `directions_are_unit_and_point_at_the_light`, `lightless_luxels_use_the_fallback_direction` |
| M5.4 | Done | `scene.rs` now draws baked lightmaps through `lighting_shader.rs` and `level/`; `level_render.rs` tests and the flythrough PNGs show light pools and shadows on metal, brick and floor. Frame cost: about 13.5 ms/frame in the game bench on the dev machine, not compared against the bump bench or measured on the i3-7100U |
| M5.5 | Done | `bump.rs` wired into the scene shader (`lighting_shader_tests`, `level_render.rs`); bump is visible on bolts and mortar in the flythrough PNGs |
| M5.6 | Partial | `specular.rs` is wired as the High quality option (default quality is high; `--quality low|medium` turn it off). Gloss comes from `assets/textures/materials.txt`, and a `name_s.png` masks the highlight per texel (M12.5). Variant D cost is still not recorded in `docs/perf/` |
| M5.7 | Partial | The engine side is done (M12.5): `load_texture` puts `name_s.png` into the alpha of the normal texels and the specular shader multiplies the highlight by it. The generator is not: `tools/gen_textures.py` emits albedo, `_n` and `_h` only, no `_s.png`, so no stock material has a mask; gloss per material comes from `assets/textures/materials.txt` |
| M5.8 | Done | `lightmap/bounce.rs`; `bounce_lights_a_room_seen_only_through_a_door`, `ambient_fills_unlit_luxels` |

## M6 Gameplay (`mscent-game`)

| ID | Status | Evidence |
|---|---|---|
| M6.1 | Done | `player.rs`; `player_tests` incl. no gimbal artifacts at 90 degrees of pitch and roll (Q/E); roll visible in the flythrough PNGs |
| M6.2 | Done | `mscent-world/src/entity.rs`; `game_tests::level_with_every_entity_loads_and_is_placed`; `assets/levels/showcase.msl` and `corridor.msl` (written by `apps/game/examples/write_levels.rs`) load in the game |
| M6.3 | Done | `enemy.rs`, `combat.rs`; `enemy_tests::loses_the_player_behind_cover_then_gives_up`, `reacquires_while_hunting_and_after_giving_up` |
| M6.4 | Done | `game.rs`; `game_tests`; end to end `apps/game/tests/playthrough.rs` and `showcase_combat.rs` (the scripted flythrough ends `Won`) |
| M6.5 | Done | `weapon.rs`; `game_tests::firing_kills_an_enemy_in_two_hits`, `shots_use_the_movement_trace_and_stop_at_walls` |
| M6.6 | Done | `hud.rs`, `menu.rs`; `hud_tests`, menu pixel tests; HUD (HP bar, enemies, goal distance, speed, fps) visible in the flythrough PNGs |

## M7 Editor

| ID | Status | Evidence |
|---|---|---|
| M7.1 | Done | `apps/editor` (xui, git dependency pinned to rev b3c8cd4): menus, tool palette, grid-size, properties and build-log panels; seen in `--selftest` snapshots. Only built and run on Windows |
| M7.2 | Done | `ui/paint2d.rs`, `ui/viewport.rs`; top, front and side views with grid, pan, zoom (editor unit tests, selftest `02-four-views.png`) |
| M7.3 | Done | `ui/view3d.rs` calls the game renderer; the lit, textured, shadowed view is visible in the four-view snapshot. "Same pixels as the game" is not asserted by a test |
| M7.4 | Done | `model/command.rs`, `pointer.rs`; create, move, resize, add/subtract toggle with undo and redo (`model/tests.rs`, `review_tests.rs`). Undo coverage is as far as those tests go, not exhaustive |
| M7.5 | Done | `model/entity.rs`; player start, light, enemy and goal tools; markers visible in all views and they survive save and reload (selftest) |
| M7.6 | Done | `apps/editor/src/build.rs` and `studio/preview.rs`: CSG, faces, portals and bake follow every edit in the background; invalid brushes are errors naming the problem (`build::tests`, `studio/preview_tests.rs`); lighting log shown in the panel |
| M7.7 | Partial | `files.rs`, `model/document.rs`: save and load of brush and entity source (`.msl`); selftest round-trips 27 brushes and 13 entities; hostile files and missing paths are errors (`files::tests`). Format versioning was not tested |
| M7.8 | Done | `studio.rs`, `studio/worker.rs`: Play runs `miniscent-game` in-process (default features off) on its own thread, Esc returns, edit then play again; selftest passes both plays and checks the document is untouched. Run headlessly only; not tried interactively with a real mouse |

## M8 LazyOS port and release

| ID | Status | Evidence |
|---|---|---|
| M8.1 | Not done | Needs LazyOS and the M1.3 findings; implement the `mscent-platform` `Platform` trait for LazyOS (no xui needed) |
| M8.2 | Not done | Needs LazyOS (`run_demo.py`, GUI launcher) |
| M8.3 | Not done | Needs the LazyOS trace channel |
| M8.4 | Not done | Needs the LazyOS boot harness |
| M8.5 | Not done | Needs the real Kaby Lake box |
| M8.6 | Not done | Needs M8.5, M7.8 and a clean Windows machine |

## M10 Level scripting

Design in `docs/scripting.md`. Nothing a script can do exists yet.

| ID | Status | Evidence |
|---|---|---|
| M10.1 | Done | `tools/check_size.py`, a Windows CI job. Run by hand: the real binary passes at 1,222,144 bytes, a lowered limit and a 1.7 MB file both fail. The CI job has not run yet. The budget went from 1.6 MB to 2.2 MB with voxel volumes, at 1,885,184 bytes, from 1,576,960 before them |
| M10.2 | Done | `labels.rs`, `format_labels.rs`, format version 3; `labels_tests.rs` (round trip, every validation error, truncation, 200,000 bit flips, shipped version 1 levels load); `game_tests::a_mover_brush_stays_out_of_the_world_bsp`; the editor keeps labels through load, delete, undo and save (`model/label_tests.rs`). Markers and zones draw in the 2D views but have no placing tool until M10.9, and that drawing was not looked at. A mover brush is simply absent from the world until M10.3 and M10.7. The release binary grew 37,888 bytes to 1,260,032, more than this much code should cost, and the cause was not found |
| M10.3 | Done | `mscent-collide`: `mover.rs`, `push.rs`, `Solids`; `mscent-game/src/mover.rs` drives poses, eased motion and hold, reverse and crush. Shots and sight stop at movers. Evidence: `tests/fuzz_movers.rs` with an independent oracle (49,964 pushes and 1,595 blocked steps in a long run, no violations), `mover_tests.rs` in both crates, and six deliberate bugs each failing those tests. Levels with no movers replay bit-exact: 60,000-step hashes over 4 seeded runs on the showcase and corridor match before and after. Frame cost is unchanged: ten runs each of `frame_bench` average a ratio of 8.447 on `main` and 8.431 here, at 1.40 ms per frame. `tools/check_frame_cost.py` fails on this machine on both, against a stored baseline of 7.61, so that baseline needs looking at. Movers still cannot be seen (M10.7) or driven by a script (M10.5), only by Rust |
| M10.1 follow-up | Done | The release profile now builds for size (`opt-level = "s"`, fat LTO, one codegen unit), with `mscent-render` and `mscent-math` kept at `opt-level = 3` because the renderer's hot paths run on its vector code. Panics still unwind, since Instant Play catches them. The game binary went from 1,298,432 to 992,768 bytes. Interleaved `frame_bench --workers 1` runs on this noisy machine (the clock swings between two modes, so ratios vary 8 to 23) gave a median 2.60 ms before and 2.53 ms after over 20 runs, and 2.69 ms before and 2.63 ms after over 30 more, so no regression. `blit_fit` 640x360 to 1920x1080 went from a best of 0.31 ms to 0.24 to 0.28 ms. A `--bake --rebake` of the showcase had medians of 336 and 340 ms over 30 interleaved runs (best 180 and 168), so `mscent-world` and `mscent-platform` stay at `s`. `tools/check_frame_cost.py` was not made to pass and `docs/perf/baseline.json` is unchanged |
| M10.4 | Done, with gaps below | `crates/mscent-script`: 3,100 lines in 15 files, none over 400, no dependencies, `unwrap`, `expect` and slice indexing denied by lint in the library. 98 tests in `tests/`: every construct in the design's language section (`language.rs`); the design's examples and `ex/*.st` against a logging stub host, asserting the whole log and the tick of each line (`examples.rs`); hostile scripts (infinite loop, string, array and value-stack bombs, recursion, fork bombs, deep nesting, 1 MiB chains and literals, `hostile.rs`, `limits.rs`); the limits and the load errors for each (`limits.rs`); messages with file and line (`errors.rs`); stale references, `Clone`, fairness, garbage collection and replay (`state.rs`); a seeded mutation fuzz of 40,000 real-script mutants and 20,000 token soups, 1.4 s (`fuzz.rs`). The chain tests run on a 512 KiB thread stack, also with the crate at opt-level 0. A real `cargo fuzz` target is in `crates/mscent-script/fuzz` (its own workspace, nightly): built and run here with the Visual Studio ASAN DLL on `PATH` (`clang_rt.asan_dynamic-x86_64.dll`; without it the target exits with `STATUS_DLL_NOT_FOUND`, and `-s none` does not link), 65,569 runs in 61 s, then 152,473 in 181 s from the example scripts as seeds, then 55,082 with inputs up to 256 KiB, no crash. That harness adds `libfuzzer-sys` as a dependency of the fuzz crate only; nothing the game builds depends on it. Mutation checks: with chains rebuilt as nested boxes the chain tests overflow the stack, and with each of the constant wrap, task cap, generation check, byte-counted heap and fairness fixes removed its test fails. Size: a probe program that loads and runs a script with 30 natives, under the workspace release profile, is 290,816 bytes against 118,784 for an empty one, so 172,032 bytes added against the 300 KB limit; the probe was not committed, and `check_size.py` cannot see the crate until M10.5 links it. Not done: nothing binds the game API, `Game::step` does not run a VM, there are no CLI flags (M10.5, M10.6). The fuzz target ran for minutes, not hours. `arithmetic_side_effects` is not lint-enforced (30 sites, all read by hand and bounded by the source or heap limits). Stack depth was tested on Windows only |
| M10.4 review | Done | Found by hostile scripts the crate's own tests missed, each now a test in `tests/review.rs`: a 1 MiB selector took 110 s to load (quadratic missing-period hint), hints were worked out past the 50-error cap (2 to 3 s), and a mostly live heap was collected on every step for the rest of the level (about 4 ms a step). The heap limit went from 16 to 4 MiB. `^` outside any `define` now ends the handler instead of being an error |
| M10.5 | Done | `mscent-game`: `script.rs`, `script_api.rs`, `script_world.rs`; `apps/game/src/scripts.rs` loads `foo.st` beside `foo.msl` and `assets/scripts/lib/*.st`. Evidence: `script_tests.rs` and `script_error_tests.rs` play each use case through `Game::step` (shooting several to open a door, a key door, hinge and lift, spawn waves, disable and enable, win and lose, a handler's subject); a scripted run replays byte for byte; 27 bad requests each error on the calling line; hostile scripts hit caps. `apps/game/tests/scripted_level.rs` runs the real binary on a level file with a script. Levels with no script replay bit-exact: 60,000-step hashes over 4 seeded runs match before and after. The binary grew 222,720 bytes to 1,215,488, 75% of the budget. The renderer was not touched, so `frame_bench` was not rerun. Pickups, props, the use key and factions are M10.8, so the keycard is a zone for now and the terminal is not playable. `say:` and script errors go to the console. Not done: the editor does not load scripts (M10.9), and no one has played a scripted level by hand |
| M10.6 | Done | `apps/game/src/agent.rs`, `trace.rs`, `scripts.rs`; `mscent-world/src/level_text.rs`. `apps/game/tests/agent_tools.rs` runs the real binary: a level from text, `--pack-level`, `--check`, then `--test` with a trace, with no build in the loop; the same test writes the same trace byte for byte; failures set the exit code and name file and line. Level text round-trips the showcase to the same bytes, and 40,000 mutations never panic. Hot reload is tested through a real session, not a real window |
| M10.7 | Done | `mscent-render/src/level/movers.rs`: a mover's faces are lit with the level at their authored pose and drawn at their current one, interpolated between steps. `tests/mover_render.rs` reads pixels: a shut door is drawn lit and textured, opening it leaves about the same picture as no door, a door buried in rock or behind the eye draws nothing, a hinge swings, every quality runs, and hostile poses do not panic. `agent_tools.rs` sees a door move through `shot:`, and PNGs of a sliding and a hinged door were looked at by eye. Cost: `docs/perf/movers.md`. Limits as designed: no baked shadows from movers, so light passes a closed door, and a face keeps the light of its authored pose |
| M10.8 | Done | `mscent-game`: `prop.rs`, `script_props.rs`, factions in `game.rs`; `apps/game/src/notices.rs` shows `say:` and script errors on screen. `script_prop_tests.rs` plays props through `Game::step`, and `agent_tools.rs` runs the keycard and the terminal end to end through `--test`. Writing that test caught a real bug: events a script raised while running were lost. A PNG of props, the use prompt and a friendly and a hostile drone was looked at. Not verified: a physical `G` key, and fights between many friendly and hostile enemies at frame rate |
| Sample | Done | `assets/levels/facility.txt`, packed to `.msl`, with `facility.st` and `facility.test.st`: shoot three switches for one door, a keycard for the next, a terminal that turns two guards friendly, a spawned wave, a goal. `apps/game/tests/sample_level.rs` plays it through the real binary and checks the packed file matches its text. `flyTo:` is a teleport, so that test does not walk through the doors; mover collision is `mover_tests`. A screenshot of the first room was looked at. Nobody has played it by hand: `cargo run --release -p miniscent-game -- --level assets/levels/facility.msl` |
| M10.9 | Done | `apps/editor/src/model/labels.rs`, `ui/props.rs` Script names panel, Edit > Make mover, Marker and Zone tools (M, Z), movers orange in the 2D views; `build.rs` loads the script beside a saved level, and a broken one refuses Play with its file and line; `studio/scene.rs` reloads it during Play. Evidence: `selftest/scripting.rs` runs through the editor's messages: names a brush and a marker, refuses a bad name, saves and reopens, is told about a broken script, then plays and samples the Play pane: door shut, opened by the script, shut again after a script saved during Play. The three shots were looked at. `label` tests cover undo, duplicates and subtract brushes. Fixing the selftest showed its face-tool index had pointed at the World tool since #6, so that check was already failing; corrected. An independent review then found, and tests now cover: Save overwriting a level with clashing names, slice and carve leaving part of a mover behind as world, stale fields after undo, joining a group wiping its tags, and an unbounded script read on the UI thread. It also found a reload error wiped from the screen on entering Play, now fixed with no test. Known and left: unapplied typing is lost on a new selection and Enter does not apply it; extruding a mover's face adds a static brush; adding a box and undoing leaves [modified] on, because `next_id` counts as a change (older than M10). Not verified: typing in the fields with a real keyboard, placing with the M and Z tools by clicking a view, and how a reload error looks on screen in Play |
| Merge with `main` | Done | Numbered M10 because M9 is the particle system (`docs/particles.md`). Movers now list once and fill per band, like the level: `mover_render::every_tiling_draws_a_door_as_one_band_on_one_worker_does`. The World tool is gone from the palette (fog is in Level settings), so Marker and Zone close it at 11 tools. `MAKE_MOVER` and main's `LEVEL_SETTINGS` had the same menu id, which the `--selftest` caught; the timing tests take main's `TickCosts` fix over the one here. Binary 1,425,920 bytes, 89% of the budget. `check_frame_cost.py` still reads 8.44 against the 7.61 baseline on this machine, as `main` does |
| Pre-PR review | Done | Fixed, each with a test that failed first: a mover blocked on its first step reversing past its own start; `onAllDestroyed:` firing once per death when several die in one step; a hostile ignoring the player in the open for a friendly behind a wall; an apostrophe in a comment breaking `--check`'s list of names; the trace's empty pose for an instant move on load; `--pack-level` reading an oversized file first. Also, loading a script no longer runs its top level twice (no test of its own). Left: picking up a group's last prop is not destroying it, so it fires no `onAllDestroyed:` though `waitDestroyed:` returns; the library falls back to the build tree's path; the editor checks script files on the UI tick |
| M10.10 | Done | `apps/editor/src/script/` (buffer, checking, completion, basic entities, samples, guide) and `ui/script_pane.rs`, on `xui-code-editor` at the rev already pinned, which brings `ropey` and `regex` into the editor only. `assets/scripts/lib/kit.st`, `assets/samples/`, `assets/guide/scripting.txt`. Evidence: `samples.rs` runs each sample through the real `check` and `run_test`; `kit_tests.rs` places every basic entity in a room and runs its script in a real `Game`, alone and all together, and undoes one; `api.rs` compares its table with `natives()`, `language_selectors()` and the script-written functions; `guide.rs` compiles every example; `buffer.rs` covers the file limit, unreadable files, failed writes and changes made outside; `selftest/script_pane.rs` drives the window with real key events and clicks, in light and dark. Its screenshots were looked at, and looking is what found the popup opening after the period that ends a statement, where Enter would have accepted a suggestion. A pre-PR review then found, and these now have tests where a unit test can reach them: a panic on non-ASCII text before `:=` in completion, Script > Open sample skipping the unsaved-edits question, F5 right after Save playing the old build, markers placed inside rock beside a door near a wall, and doors fitted to the wrong one of two stacked rooms. The owner then reported the editor far less responsive than `657a486`. `--ui-timing` measured a re-flow at 115 ms against 0.6 ms: the guide's pages, mounted though hidden, were about 400 widgets, and `FlowText` measures word by word on every pass. The guide is now one code editor, and a re-flow takes 2.2 ms with the pane closed and 3.1 ms with it open, guide included. The owner's own door did not open, and a copy of the level run through `--test` showed why: the door had been moved after placing, and `door_1_area` stayed in the far room. A mover's `_area` zone now follows it through drags, resizes and typed bounds, as one undo step, and the problem list warns when one does not reach its mover (`script/area.rs`, which runs a moved door in the real game). The pane can also live in its own window (`selftest/script_pane.rs`, with the window's own snapshot). The headless backend has no wake, so a second window's messages do not run there and the selftest cannot follow one from the window to the editor. `selftest/materials.rs` fails on `657a486` too (`sizes 10 became 6`). Not verified: a physical keyboard, a real second window and that forwarding, F5 giving the game the pointer from there, the window's close button, the dialog itself in a real window, or a person trying it. Known and left: a level's own unsaved changes are still not asked about on New or Open, only the script's; undoing a placed entity leaves its script lines, which the problem list then reports; the library falls back to the build tree's path for a level outside an `assets` tree, so outside a checkout such a level cannot find `kit.st`; a mistake that only shows when a handler runs is reported on the HUD in Play, not in the problem list |

## M11 Voxel volumes

Design in [voxels.md](voxels.md). The game builds the volumes' surfaces and collides with them (M11.6), and the editor edits them with the tools of M11.9; the game does not draw them yet.

| ID | Status | Evidence |
|---|---|---|
| M11.1 | Done | `mscent-world/src/voxel/`: `surface_tests` (closed across chunks and any mix of detail levels, border vertices keep their bits, only reported chunks change), `resolve_tests` (an untouched volume leaves the world as solid as it was, moving a brush keeps the sculpting, the surface runs past the box), `dab_tests`; `crates/mscent-bench/examples/voxel_bench.rs`; `crates/mscent-render/examples/voxel_shot.rs` draws five views with 0 hole pixels through `draw_polygon` with stand-in light. Dabs are Add, Carve, Smooth and Paint; Flatten and Roughen come with the tool in M11.7. Not measured on the i3-7100U |
| M11.2 | Done | `voxel/wire.rs`, section 5 of level format version 4; `voxel/wire_tests.rs`: round trip, versions 1 to 3 load, bit flips and truncation never panic and whatever loads can be meshed, forged runs refused, detail chunks charged before allocation. The editor document keeps volumes across load and save. `voxel_bench` reports 0.84 MiB and 1.2 ms to read for the 256-chunk cave. The writer does not enforce the read limits yet (M11.7) |
| M11.3 | Done | `level.csg_brushes()` adds a no-draw shell per subtract volume; `voxel/world.rs` meshes a level against its BSP, drops triangles no air can see and notes each chunk's leaves, and chunk bounds feed node bounds. Samples nobody painted take the brush's material. `voxel/world_tests.rs`: wall material, a tunnel across two touching volumes (either cell size) has no wall. `voxel/fill_tests.rs`: a seeded room has no hole and about the same look, a volume with only shell faces renders |
| M11.4 | Done with a known miss | `mscent-render/src/voxel/`: list, flood, fill, blend, shader. `fill_tests.rs`: no sky pixel over 1000 poses at three sizes, with level of detail and through the near plane; same picture on any number of workers. `tests/voxel_frame.rs`: a frame allocates nothing of its own. **Miss:** on the dense gyroid at High a voxel pixel costs 51 to 57 ns against 30 to 50 ns for a brush pixel (`voxel_bench_frame`, quiet machine); the design asks for no more. Its 40 000 small triangles set the floor, and bump at every level of detail and fading projections were added for the look. `check_frame_cost.py` has no voxel scene yet |
| M11.5 | Done | Light on voxel surfaces. `mscent-world/src/lightmap/`: `occluders.rs` (the BSP, then a grid walk over each volume's triangles in `voxel_rays.rs`; both callers of `segment_clear` use it), `cells.rs`, `cell_sites.rs` (one 9 byte record per surface cell, keyed by the global cell, so chunks share border cells), `cells_bounce.rs`. `voxel_light_tests`: a ray through a pillar is blocked only with the volume as an occluder; a pillar shadows the floor; a plate shadows the pillar; the root of a pillar is not black (the test checks that root cells did start inside the floor); one chunk lit alone gets the records the whole bake gives it; border cells resolve to one record; painting keeps `shape_key`, carving changes it; the records round trip and forged counts are refused. `tests/voxel_light.rs` (pixels, flat grey): a voxel pillar darkens the brush floor behind it to 4% of the lit value and a brush plate darkens the voxel pillar to 2% (each test fails with its half of `Occluders` turned off); brush and voxel floors differ by at most 0.7% in mean luminance either side of the seam; a draft lights and shadows voxel floors. `tests/voxel_light_cache.rs`: painting keeps `light_key` and bakes nothing, carving changes it and bakes; a file of another shape is stale; forged counts are refused; any bytes in the records load and render. `tests/crawl.rs` follows a shadow edge on a voxel floor: contour crawl 0.02 px, luminance noise 0.26 levels (the brush goals are 0.06 and 0.9). A level without volumes bakes the luxels it did: the `bake_cache_review` fingerprint is unchanged and only `store::VERSION` moved, to 5. Bake time of a 295k triangle cave is in `docs/voxels.md`. Not done: the stroke-time refinement (editor work); bounce from voxels onto brush luxels; a cell's lifted point is tested against brush solid only |
| M11.6 | Done | `mscent-collide/src/voxel.rs` (sweep, same contact model as `cell.rs`), `Solids::with_voxels`, `mscent-world/src/voxel/{index,query}.rs`, `Arena::voxels`. `tests/fuzz.rs` runs both properties on levels with a carved and an Add volume (with the voxels left out of `Solids` both fail); `tests/voxel.rs`: a slide from a brush floor onto voxel rock and along a brush wall through a volume keeps its speed every step, a carved tunnel is walked and uncarved rock stops the sphere, a replay (level built afresh each time) is bit-exact, an embedded sphere can leave but not dig in, empty and hostile worlds (NaN, bad indices, zero and NaN chunk sizes) do not panic; `voxel/query_tests.rs`: `is_solid` agrees with the field at 5000+ samples away from the surface, deep rock answers from the chunk verdict (fails without it), the index returns each triangle once and stays in budget; `game/voxel_tests.rs`: the player stops at voxel rock, nothing spawns inside it, a replay through it is bit-exact. `voxel_bench`: a sweep at radius 0.5 about 12 µs in a 220k triangle cave, 33 µs for `slide_move`. Not measured on the i3-7100U. The particle system asked the BSP alone until the change recorded under M11.8 |
| M11.7 | Done | Volumes are brush-list entries (`Shape::Volume`). `model/stroke.rs`: strokes on a thread. `studio/live.rs`: voxel-only changes skip CSG and bake. `studio/live_tests.rs`: a stroke starts no CSG job, UI tick under 1 ms at the 90th percentile with radius 4 smoothing. `selftest/voxel.rs`: stroke, undo to the same picture, redo, save, reopen, play. Saving refuses a level the loader would refuse |
| M11.8 | Done | `mscent-game/src/destruct.rs`. `destruct_tests.rs`: collision follows a shot, two meshes a tick and one whole swap, replay bit-exact, restart restores. Rock is the destructible material. Particles collide with the carved rock too: `fx` reads `Arena::fx_world()`, which holds `Arena::voxels`, so a new surface is seen the tick the swap lands (`fx_voxel_tests`, see voxels.md) |
| M11.9 | Done | `apps/editor/src/model/generators/` (stalactites, mushrooms, coral, trees, hive, `field.rs` for Fill), `sculptor.rs`, `growth.rs`, `ui/voxelpanel.rs`. `mscent-world::voxel` gained Flatten and Roughen (`DabOp`), `noise3`, `capture` and `stamp` (Copy and Stamp), `Prim::spheroid` and `Prim::hex_prism`. Evidence: `voxel/tools_tests.rs` (flatten cuts a hump and fills a pit, roughen is seeded, a stamp reproduces the copy, hostile shapes and stamps), `generators/tests.rs` (same seed same plan, bounded, nothing under a cell, bad sites give nothing, normals), `model/growth_tests.rs` (each tool through the editor state: one undo step, held until Commit, re-roll by seed, Esc drops it, Fill), `selftest/voxel.rs` (a generator click holds, Commit records). `generators/showcase_tests.rs` builds one level per use case with the generators: the same seed gives identical `format::write` bytes and another seed differs, and each renders within `MAX_VOXEL_TRIS` after 40 frames with no fuchsia pixel from its pose in air. Triangles at 640x360: hive 9254, coral 14168, mushrooms 3541, tree 4674, cave 16662 of 25000. The levels are `assets/levels/voxel-*.msl` (`MSCENT_BLESS=1 cargo test -p mscent-editor showcase` rewrites them) and `voxel_shot` draws them; the pictures were looked at and the shapes iterated. Not done: the wireframe ghost (a held, real result with a re-rolling wheel stands in; see voxels.md), snapping each drip to a curved surface, and comparing the saved `.msl` files with a fresh build, because `sin` and `cos` may differ in the last place between platforms. Not measured: Fill and Stamp on large volumes, and the panel with a real mouse |
| Merge with `main` | Done | Numbered M11 because M9 is particles and M10 scripting. Scripting took section 4 and version 3, so volumes moved to section 5 and version 4 |
| Pre-PR review | Done | Fixed, with tests: the editor moved a volume in the CSG order on save (`model/csg_order_tests.rs`); a volume tied with a brush's order loaded; Smooth gave new solid a stale material (that test failed with the fix turned off); shapes past the range of brushes could overflow to NaN. `--dump-level` refuses volumes rather than lose them. Left for M11.3 and written into its done-when: carved rock in a `Subtract` volume shows material 0, and touching volumes do not see each other. Left: the first dab after a snapshot copies the chunk table (at most 4096 entries, once per snapshot, not measured) |

## Deviations from the plan

- Platform layer: the game does not use xui. `mscent-platform` is a small trait with a desktop (winit + softbuffer) and a headless backend; xui is used only by `apps/editor`. A LazyOS backend is not written.
- `mscent-bench` was added as a crate for `bump_bench`.
- Core crates are not declared `no_std`; they use `std` only. `mscent-platform` (winit, softbuffer, optional) and the editor (xui) are the only crates with external dependencies.
- Textures are decoded at load time by an in-repo PNG decoder, not at build time.
- The "fuzz target" for the level format is a seeded in-process test.
- Still open: `_s` gloss textures from the generator (M5.7; the engine reads them since M12.5), perf numbers on the i3-7100U, all LazyOS work (M1.3, M1.4, M8.x).
