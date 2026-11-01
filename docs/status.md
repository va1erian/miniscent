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
# scripted editor session: load, edit, save, reload, build, four views, play, Esc, edit again
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
  upstream (xui#313); the editor now builds against xui `b3c8cd4` and the vendored copy of that fix
  is gone. Not verified: a real window moved between monitors of different DPI (cannot be done here).
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
- **F5 and F7.** Root cause: xui's winit keymap only produced `Key::F1` to `F4`, so a real window
  never raised F5 to F12 (offscreen tests inject events and hid it).
  `docs/xui-patches/0002-map-f5-to-f12.patch` adds the arms (vendored copy, with a unit test; xui#316
  is still open upstream, so `vendor/xui-canvas` stays until it is fixed). The
  editor also binds Ctrl+B (build) and Ctrl+R (play) as a fallback. `selftest/keys.rs` injects
  `KeyDown` F7, F5 and Esc events through the UI and checks the build, the play and the return;
  `ui/app/keys_tests.rs` does the same. Not verified: a physical keyboard.
- **Icon toolbar.** The tool palette is square icon-only toggle buttons with tooltips
  (`ui/toolbar.rs`, `ui/icons.rs`: xui's Lucide icons plus a drawn person, bulb and flag); the
  selected tool is filled with the accent colour. The properties and build-log columns were
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
  In the editor a texture-mapping edit on the showcase reports "lightmap kept" and takes 31 to 50 ms
  (selftest). I did not record a separate bake-from-scratch time; a whole showcase build is about
  0.1 s in release.
- **Threaded Instant Play.** Play runs on a render thread (`studio/worker.rs`): input goes in over a
  bounded `try_send` channel, frames come out through a one-slot mailbox read with `try_lock`, stale
  frames are dropped, and a slow scene lowers the adaptive resolution instead of stalling the UI. A
  panic in the scene is caught and reported. `studio/tests.rs`: with a scene that needs 100 ms per
  frame, 200 UI ticks had a worst tick of 6.7 microseconds (mean 1.3); the scene made 6 frames and
  the UI showed 6. `studio/race_tests.rs` covers start and stop races. Not verified: responsiveness
  of a real window under load.
- **Hammer-style UV controls.** See the next section.

Not verified: any real window, keyboard or multi-monitor use, the AVX2 paths on the i3-7100U, any
LazyOS run, and `tools/check_frame_cost.py` was not rerun.

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
| M3.1 | Partial | `texture.rs` (`load_texture`, `MaterialTable`, gloss) with `png.rs` and `inflate.rs`; decoding happens at load time, not build time. `assets/textures` has albedo, `_n` and `_h` |
| M3.2 | Done | `camera.rs`, `clip.rs` and their tests |
| M3.3 | Done | `raster.rs`; `tests/snapshot.rs::brick_quad_facing_the_camera` against `tests/ref/brick_quad.ppm` |
| M3.4 | Done | `scene.rs` and `scene/walk.rs`; `scene_tests::flythrough_matches_brute_force_with_no_doubled_faces` |
| M3.5 | Done | `snapshot::brick_wall_in_perspective_with_fog`, `flat_lit_fallback_scales_brightness`, `framebuffer_tests::sky_gradient_runs_from_top_to_bottom_colour`; the level renderer draws sky, fog and lit surfaces in the flythrough PNGs |
| M3.6 | Partial | `tiles.rs`, `simd.rs`, `simd_x86.rs`; `tiles_tests::one_and_many_workers_give_identical_pixels`. Scaling not measured on multiple cores, AVX2 path unmeasured, no-syscall property not asserted |
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
| M5.6 | Partial | `specular.rs` is wired as the High quality option (default quality is high; `--quality low|medium` turn it off). Gloss still comes from `default_gloss`, not `_s` textures (M5.7). Variant D cost is still not recorded in `docs/perf/` |
| M5.7 | Not done | `tools/gen_textures.py` emits albedo, `_n` and `_h` only, no `_s.png`; gloss comes from `default_gloss` in `texture.rs` instead |
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
| M7.6 | Done | `apps/editor/src/build.rs`: CSG, faces, portals and bake with a log; invalid brushes and empty levels are errors naming the problem (`build::tests`); log shown in the panel |
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

## Deviations from the plan

- Platform layer: the game does not use xui. `mscent-platform` is a small trait with a desktop (winit + softbuffer) and a headless backend; xui is used only by `apps/editor`. A LazyOS backend is not written.
- `mscent-bench` was added as a crate for `bump_bench`.
- Core crates are not declared `no_std`; they use `std` only. `mscent-platform` (winit, softbuffer, optional) and the editor (xui) are the only crates with external dependencies.
- Textures are decoded at load time by an in-repo PNG decoder, not at build time.
- The "fuzz target" for the level format is a seeded in-process test.
- The editor builds against a vendored `xui-canvas` (`vendor/xui-canvas`, `[patch]` in the root `Cargo.toml`) with one fix not yet upstream (F5 to F12 keymap, xui#316); the patch is in `docs/xui-patches/`.
- Still open: `_s` gloss textures (M5.7), perf numbers on the i3-7100U, all LazyOS work (M1.3, M1.4, M8.x).
