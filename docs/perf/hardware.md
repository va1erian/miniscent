# What a frame costs on the GPU

The hardware renderer against the budgets in [hardware-renderer.md](../hardware-renderer.md)
("Costs and budget"), on the dev machine: an RTX 5070 Ti, 24 hardware threads. The reference
machine, an i3-7100U with its HD 620, has not run any of it.

## The frame

`cargo run --release -p mscent-gpu --example frame_cost`: three levels at 3840x2160, turning
a full circle over 240 frames.

- **CPU side:** listing, encoding and submitting a frame with nothing read back, as the
  direct route does before it presents.
- **Done:** the time until the GPU has finished, measured from the same start.
- **Not included:** the first frame, which makes the targets, is given apart.

Milliseconds, median, 95th percentile and worst of the other 239 frames:

| Level | Finish | CPU side | Done | First frame, CPU / done |
|---|---|---|---|---|
| showcase | Fine (High) | 0.07, 0.11, 0.22 | 0.43, 0.72, 0.91 | 3.91 / 6.79 |
| showcase | Reference | 0.05, 0.10, 0.18 | 0.28, 0.54, 0.76 | 2.86 / 4.64 |
| facility | Fine (High) | 0.06, 0.11, 0.13 | 0.42, 0.72, 0.93 | 5.44 / 7.09 |
| facility | Reference | 0.05, 0.10, 0.14 | 0.27, 0.45, 0.72 | 2.71 / 4.33 |
| voxel-cave | Fine (High) | 0.05, 0.09, 0.18 | 0.23, 0.40, 0.60 | 5.45 / 6.92 |
| voxel-cave | Reference | 0.04, 0.07, 0.16 | 0.14, 0.33, 0.66 | 2.77 / 4.58 |

Against the budgets:

| Budget | Target | Measured |
|---|---|---|
| CPU side of a frame | under 1.5 ms | 0.22 ms worst |
| GPU side | under 4 ms | 0.93 ms worst |
| Worst frame, after the first | under 8.3 ms | 0.93 ms; the first frame of a level is up to 7.1 ms |
| Level upload, warm bake | under 100 ms | 17 to 27 ms. The first level after the driver's shader cache was emptied took 121 ms, as its pipelines compiled |
| Input latency | a frame latency of 1 | `desired_maximum_frame_latency: 1`, and a present mode that does not wait for the display |
| Allocations a frame | no more than software's | Not measured: a counting allocator is `unsafe`, which `mscent-gpu` forbids |

## The game in a window

Measured from the game's own report of its render call (`app::run_with`), on the dev machine's
display:

- **Direct route, 4K window:** a 6 s run at a 1920x1080 internal size in a 3840x2160 window.
  - Software: 6.75 ms a frame on average and 7.33 ms at the 95th percentile, before its copy
    to the window.
  - GPU: 4.53 ms at the 95th percentile, composite and present included.
- **Window-sized world:** at 1920x1080, the GPU draws a 1920x1080 world under a 640x360 HUD.
  The render call is 0.80 ms at the 95th percentile, mostly CPU time, as the GPU works behind
  it.
- **Readback route, headless:** the flythrough at 1920x1080 over 1861 frames.
  - Software: 8.91 ms on average and 11.20 ms at the 95th percentile, its adaptive scale down
    to 625.
  - GPU: 5.06 and 6.85 ms at full scale, the fine finish included.

## The particle budget's depth

The particle budget reads a depth the session draws on the CPU each frame. It is the visible
faces, flat, at a quarter of the frame and at most 160 pixels wide. Drawing it costs:

| Size | Cost |
|---|---|
| 160x90 | 0.16 ms |
| 320x180 | 0.52 ms |
| 960x540 | 4.0 ms |

This replaced the last frame's own depth, so it changes what software thins over budget.
Over 90 frames of a fight at 640x360, sprites and streaks drawn:

| Fight | Before | After |
|---|---|---|
| Eight blasts | 1,915 and 8,025 | 1,912 and 8,008 |
| 24 blasts | 2,644 and 6,772 | 2,643 and 6,763 |
| No blasts | 22 and 22 | 22 and 22 |

## Resolution on a slow GPU

The adaptive resolution reads a frame's cost from its render call, which on the direct route
includes showing it. When the GPU falls behind, the next window texture waits, at a frame
latency of 1, so the cost rises and the world is drawn smaller. `apps/game/tests/slow_gpu.rs`
shows that with a stand-in that takes 30 ms a frame. No slow GPU has run it.

## Projected shadows

`frame_cost` puts two dynamic lights beside the eye and six stock sentries round it, so
that the shadows have lights and casters; `noshadows` after it switches only the shadows
off, `plain` both effects. 3840x2160, High (Fine), median over 240 frames of the time until
the GPU is done, in milliseconds, RTX 5070 Ti, the other 239 frames after the first:

| Level | Shadows on | `noshadows` | `plain` | Cost of shadows |
|---|---|---|---|---|
| showcase | 1.89 (p95 2.19) | 0.76 (0.99) | 0.67 | 1.13 |
| facility | 1.91 (p95 2.23) | 0.66 (0.98) | 0.67 | 1.25 |
| voxel-cave | 0.58 (p95 0.80) | 0.42 (0.62) | | 0.16 |

The budget in [hardware-effects.md](../hardware-effects.md) is 1.5 ms or less for two lights
(it said 1024 a side; this is 512). It is met, with the lights near the eye and so over the
most of the screen; the cost is nearly all in the receivers (a blocker search of 12 taps and
a filter of 32 on each lit pixel that a light with a map reaches), not in the six passes. The
CPU side is 0.1 ms more. Medium, which has no shadows, is unchanged (0.53 against 0.54).
Memory: the maps are 2 lights x 6 faces x 512 x 512 x 4 bytes, 12.6 MB, and a 1 MB depth.

Cheaper receivers are the first thing to reach for if this has to come down: fewer taps
where the kernel is under 3 texels, or a map that is kept while its light and casters do
not move.

### Seeded

Each bug put in by hand, the tests run, then taken out again:

| Bug | Caught by |
|---|---|
| Shadow ignores its switch (pick, and the shader's check) | `tests/shadow_off.rs` |
| Filter taps cut to 1 and the kernel to 0 (no PCF) | `the_edge_is_soft_and_steady...`: penumbra 0.6 px against 8.8, limit 6 |
| Cube faces +X and -X swapped | `an_actor_casts_a_shadow_and_so_does_a_mover` only |
| Cube faces +Y and -Y swapped, or +Z and -Z | `a_shadow_falls_right_for_a_light_on_every_side_of_the_box` |
| No casters drawn | 7 of the 8 tests in `tests/shadows.rs` |
| The shadow multiplies the baked light too | `the_shadow_takes_only_the_dynamic_light_and_leaves_the_baked` |
| **No bias at all** (normal offset, constant, slope, and the receiver's plane taken at a point) | **nothing**: every test passes |

The last is not a bug the tests can see yet. No receiver in this slice is also a caster, so
nothing is ever at the depth of what shades it, and bias has nothing to do. The follow-up
that makes static faces casters needs its acne test, with this bug seeded, in the same
change.
