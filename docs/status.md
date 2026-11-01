# Implementation status

Checked against the code. `cargo test --workspace` passes: collide 45 unit + 3 fuzz, game 38,
math 40, render 128 unit + 2 stress + 4 snapshot + 1 tiled scene, world 85. `tools/check_file_length.py`,
`tools/check_docs.py` and `tools/check_frame_cost.py` (ratio 7.69 vs baseline 7.61, +1.1%) pass.
Not verified in this final pass: clippy and fmt were not rerun; AVX2 paths were not measured on
the i3-7100U. Everything runs on one Windows dev machine; there is no window, platform layer or editor.

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
| M1.1 | Not done | Needs xui; no `mscent-platform` crate exists |
| M1.2 | Not done | Needs xui (depends on M1.1); the game has an abstract `input.rs` only |
| M1.3 | Not done | Needs the LazyOS source tree and the physical box |
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
| M3.5 | Partial | `snapshot::brick_wall_in_perspective_with_fog`, `flat_lit_fallback_scales_brightness`, `framebuffer_tests::sky_gradient_runs_from_top_to_bottom_colour`; scene traversal itself draws flat-lit (see M5.4) |
| M3.6 | Partial | `tiles.rs`, `simd.rs`, `simd_x86.rs`; `tiles_tests::one_and_many_workers_give_identical_pixels`. Scaling not measured on multiple cores, AVX2 path unmeasured, no-syscall property not asserted |
| M3.7 | Partial | `resolution.rs` controller and `upscale_nearest` (`resolution_tests::drops_when_cost_exceeds_budget`, `converges_under_budget_and_recovers_when_load_falls`); not wired to a window |
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
| M5.4 | Partial | Span-evaluated lightmap shading is in `lighting_shader.rs` and `shade.rs` with tests, but the BSP scene renderer (`scene.rs`) does not draw baked lightmaps, so bake output is not connected to a rendered level. Frame cost vs the bench not measured |
| M5.5 | Partial | `bump.rs`; `lighting_shader_tests::bumped_mortar_is_darker_and_bricks_match_flat`, `real_brick_bump_changes_exactly_the_non_flat_texels`; same wiring gap as M5.4 |
| M5.6 | Partial | `specular.rs`; `zero_gloss_specular_matches_the_diffuse_path_exactly`. Variant D exists in `bump_bench`, but its cost is not recorded in `docs/perf/` |
| M5.7 | Not done | `tools/gen_textures.py` emits albedo, `_n` and `_h` only, no `_s.png`; gloss comes from `default_gloss` in `texture.rs` instead |
| M5.8 | Done | `lightmap/bounce.rs`; `bounce_lights_a_room_seen_only_through_a_door`, `ambient_fills_unlit_luxels` |

## M6 Gameplay (`mscent-game`)

| ID | Status | Evidence |
|---|---|---|
| M6.1 | Done | `player.rs`; `player_tests::pitching_through_ninety_degrees_has_no_gimbal_artifacts` (quaternions in `mscent-math`) |
| M6.2 | Done | `mscent-world/src/entity.rs`; `game_tests::level_with_every_entity_loads_and_is_placed` |
| M6.3 | Done | `enemy.rs`, `combat.rs`; `enemy_tests::loses_the_player_behind_cover_then_gives_up`, `reacquires_while_hunting_and_after_giving_up` |
| M6.4 | Done | `game.rs`; `game_tests::reaching_the_goal_wins_and_freezes_the_game`, `dying_loses`, `restart_recovers_from_loss_and_win_and_replays_identically` |
| M6.5 | Done | `weapon.rs`; `game_tests::firing_kills_an_enemy_in_two_hits`, `shots_use_the_movement_trace_and_stop_at_walls` |
| M6.6 | Done | `hud.rs`, `menu.rs`; `hud_tests::healthy_hud_pixels`, `menu_pixels_and_hidden_menu_draws_nothing` (hash and count based pixel checks) |

## M7 Editor

| ID | Status | Evidence |
|---|---|---|
| M7.1 | Not done | Needs xui; no `apps/editor` |
| M7.2 | Not done | Needs xui |
| M7.3 | Not done | Needs the xui editor shell (the renderer is ready) |
| M7.4 | Not done | Needs the xui editor shell (`brush.rs` validation is ready) |
| M7.5 | Not done | Needs the xui editor shell |
| M7.6 | Not done | Needs the editor; the CSG, portal and bake library calls exist |
| M7.7 | Not done | Needs the editor; `format.rs` stores compiled levels, not brush source |
| M7.8 | Not done | Needs the editor and a platform window |

## M8 LazyOS port and release

| ID | Status | Evidence |
|---|---|---|
| M8.1 | Not done | Needs LazyOS and the M1.3 findings |
| M8.2 | Not done | Needs LazyOS (`run_demo.py`, GUI launcher) |
| M8.3 | Not done | Needs the LazyOS trace channel |
| M8.4 | Not done | Needs the LazyOS boot harness |
| M8.5 | Not done | Needs the real Kaby Lake box |
| M8.6 | Not done | Needs M8.5, M7.8 and a clean Windows machine |

## Deviations from the plan

- No `mscent-platform` crate or `apps/` directory exists. `mscent-bench` was added as a crate for `bump_bench`.
- Core crates are not declared `no_std`; they use `std` only and no external crates.
- Textures are decoded at load time by an in-repo PNG decoder, not at build time.
- The "fuzz target" for the level format is a seeded in-process test.
- The playable pipeline (bake output drawn by the BSP scene renderer) is not integrated; lightmap, bump and specular shading are verified in isolation.
