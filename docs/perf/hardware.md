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
