# What a dynamic light costs on the GPU

M17.1 of [dynamic-lights.md](../dynamic-lights.md): the cost of each light before anything is
built, on the dev machine (an RTX 5070 Ti, 24 hardware threads, Windows). The reference
machine, an i3-7100U with its HD 620, has not run any of it, and is where this matters most.

## Method

`cargo run --release -p mscent-gpu --example light_cost -- [options]`:

- Options: `--levels`, `--sizes`, `--counts`, `--radius`, `--runs`. Defaults are the tables here.
- Each case turns a full circle over 60 frames at High (`Finish::Fine`, 4x multisampled), the
  lights scattered deterministically within 10 units of the eye, each in open space (it reaches
  a face). All lights have radius 6 unless said, and colour (1.0, 0.8, 0.5).
- **CPU side:** `draw_unread` returning, as the direct route does before it presents.
- **Done:** from the same start until `Gpu::wait` returns.
- **GPU queued:** 30 frames submitted back to back, then one wait, divided by 30. The CPU
  encoding overlaps, so this is the GPU's own cost per frame. It is the number to read. Nothing
  is read back anywhere.
- Each number is the median of 60 frames; the table shows the median of 5 such runs.
- **Place:** `DynLight::place`, which is `Scene::light_reach` plus checks, timed a light at a
  time, median.
- The example works at the shipped cap too, and says how many lights it drew. Past
  `MAX_DYN_LIGHTS` the extra lights are ignored.

### The throwaway build

The cap is 4 in five places, so the tables came from a build that was not committed (a patch
of about 30 lines). To redo it:

1. `MAX_DYN_LIGHTS = 256` in `mscent-render/src/dynlight.rs`; the assert in
   `mscent-gpu/src/dyn_lights.rs` to 256; both arrays in `Frame` in `common.wgsl` to 256.
2. `dyn_lights::mask` returns 1 when any light is lit and ignores `reaches`, so every face and
   every voxel chunk takes every light, with no wall rule.
3. In `face.wgsl` `dynamic_light` and `voxel.wgsl` `with_dynamic`, replace the mask loop with
   `for i in 0..256 { let l = frame.lights[i]; if l.w <= 0.0 { break; } ... }` (unused slots
   have radius 0, so the loop ends at the last light).
4. To split faces from voxels, skip the `FaceMasks::write` mask or have the voxel `reaching`
   return 0.

This is the "raise the cap and loop every light per pixel" option of the plan, the worst case
for the shader: nothing is skipped but by the distance test inside the loop.

## Levels

The repository's levels are small, so these are light scenes, not hard ones. The faces column
is faces drawn in the first frame.

| Level | Faces in level | Drawn | Voxel triangles |
|---|---|---|---|
| showcase | 122 | 21 | 0 |
| facility | 18 | 6 | 0 |
| voxel-cave | 5 | 0 | 158 |
| voxel-coral | 6 | 6 | 19,126 |

voxel-cave draws almost nothing, so it only shows a floor. voxel-coral is the voxel level that
counts.

## GPU time a frame, 4 to 256 lights (milliseconds, median of 5 runs)

GPU queued, radius 6:

| Level | Size | 0 | 4 | 16 | 64 | 256 | Per light, from 64 and 256 |
|---|---|---|---|---|---|---|---|
| showcase | 1920x1080 | 0.104 | 0.124 | 0.181 | 0.441 | 1.438 | 5.2 us |
| showcase | 3840x2160 | 0.362 | 0.431 | 0.668 | 1.626 | 5.417 | 19.8 us |
| facility | 1920x1080 | 0.109 | 0.116 | 0.168 | 0.384 | 1.267 | 4.5 us |
| facility | 3840x2160 | 0.363 | 0.444 | 0.642 | 1.502 | 4.799 | 17.4 us |
| voxel-coral | 1920x1080 | 0.111 | 0.122 | 0.160 | 0.306 | 0.901 | 3.1 us |
| voxel-coral | 3840x2160 | 0.392 | 0.428 | 0.542 | 1.105 | 3.268 | 11.2 us |
| voxel-cave | 1920x1080 | 0.063 | 0.056 | 0.051 | 0.071 | 0.105 | 0.2 us |
| voxel-cave | 3840x2160 | 0.154 | 0.167 | 0.182 | 0.204 | 0.334 | 0.7 us |

Done (GPU finished, from the start of the CPU work) and CPU side, 3840x2160:

| Level | Lights | CPU side | Done |
|---|---|---|---|
| showcase | 0 | 0.089 | 0.456 |
| showcase | 4 | 0.091 | 0.526 |
| showcase | 16 | 0.094 | 0.717 |
| showcase | 64 | 0.124 | 1.657 |
| showcase | 256 | 0.208 | 5.605 |
| voxel-coral | 0 | 0.098 | 0.490 |
| voxel-coral | 4 | 0.099 | 0.524 |
| voxel-coral | 16 | 0.100 | 0.633 |
| voxel-coral | 64 | 0.118 | 1.200 |
| voxel-coral | 256 | 0.178 | 3.135 |

- The GPU cost is linear in lights (the 4, 16, 64 and 256 columns rise by about the same
  amount per light) and about linear in pixels: 3.7 times the pixels of 1080p cost 3.4 to 3.8
  times the time.
- The CPU side grows from 0.09 to 0.21 ms at 256 lights, about 0.5 us a light: the uniform
  write and the mask. It is nothing next to the GPU side.
- Against the budgets in [hardware-renderer.md](../hardware-renderer.md) (GPU under 4 ms, CPU
  under 1.5 ms): at 4K brute force passes up to 64 lights (1.6 ms) and fails at 256 (5.4 ms of
  the 4 ms) on this GPU. At 1080p 256 lights cost 1.4 ms.

## Where the cost per light is

**Faces: the per-pixel loop.** The face loop is `dynamic_light()` in `face.wgsl`, run for each
shaded pixel. Three checks:

| Radius (4K, 256 lights) | Showcase GPU queued | Per light | voxel-coral | Per light |
|---|---|---|---|---|
| 0.5 (almost nothing in reach) | 4.853 ms | 17.5 us | 3.219 ms | 11.1 us |
| 6 | 5.417 ms | 19.8 us | 3.268 ms | 11.2 us |
| 24 | 7.222 ms | 26.8 us | 4.207 ms | 14.9 us |

A light that reaches almost no pixel costs 88% of one that reaches many (showcase). So the cost
is the loop itself, the tangent frame dots, the `length` and the compare, run for every light
at every pixel, not the shading of the light that passes. Skipping a light for a pixel it does
not reach is where the saving is, which is what a per-cluster list does.

**Voxels: per vertex, and small.** `with_dynamic()` runs in the vertex shader, once for each of
a chunk's vertices. On voxel-coral, with the face lights switched off, 256 lights add 0.04 ms
at 1080p (0.108 to 0.149) and 0.04 ms at 4K (0.339 to 0.382), against 2.5 ms for the faces alone
at 4K (0.393 to 2.910). It does not grow with the window. 19,126 triangles are about 57,000 vertices, so 256 lights are
15 million light evaluations, which this GPU does not notice. Almost all of voxel-coral's cost
above is its six faces (the cave's walls), not the voxels. A voxel level with several hundred
thousand triangles would be worse, and is not in the repository.

**Overdraw: low in these levels, not proven.** Faces are listed front to back and depth tested,
so a covered pixel is mostly rejected before it is shaded. The shader has a `discard` for
cutouts, which can make some drivers shade before the depth test. A rough check, not a
measurement: 19.8 us a light over 8.3 million pixels is 2.4 ps a pixel a light, and about 30
flops of loop body at the card's ~20 Tflops gives 12 us, so the card shades something like 1.5
times the screen's pixels, including multisample edges. No counter was added to the renderer
for this.

## Placing a light

`DynLight::place` (the portal flood of `Scene::light_reach`), median per light:

| Level | Faces | place, 256 lights |
|---|---|---|
| showcase | 122 | 0.2 to 0.3 us |
| facility | 18 | 0.1 to 0.2 us |
| voxel-coral | 6 | 0.1 us |
| facing | 168 | 0.4 us |

The first light of a run costs about 1 us (cold caches). A tiny level floods a handful of
leaves; a large level with many portals in a radius of 64 will be many times slower. No such
level is in the repository, so this is a floor, not the cost to plan on. At 256 lights even 100
us a light would be 25 ms, so a light that moves a lot must not re-flood each frame, which M17.4
already says (re-flood on leaf crossing).

## Cluster grid at 4K

16x16-pixel tiles, as in the plan:

| Size | Tiles | Slices | Clusters | Lists at 32 a cluster, u32 | Counts and offsets (8 bytes) |
|---|---|---|---|---|---|
| 1920x1080 | 8,160 | 16 | 130,560 | 16.9 MiB | 1.0 MiB |
| 1920x1080 | 8,160 | 24 | 195,840 | 25.4 MiB | 1.5 MiB |
| 3840x2160 | 32,400 | 16 | 518,400 | 67.2 MiB | 4.0 MiB |
| 3840x2160 | 32,400 | 24 | 777,600 | 100.9 MiB | 5.9 MiB |

The plan's 32-entry cap with fixed slots is too much memory at 4K and a 100 MiB buffer cannot
be rewritten each frame. Variable-length lists (a count and an offset a cluster, entries packed)
have only the entries that exist: at 256 lights even a few hundred clusters each is under a
million entries (4 MB). The 4 to 6 MiB of headers still has to be cleared and written each
frame; 32x32 tiles at 4K (8,100 tiles, 130,000 to 194,000 clusters, 1 to 1.5 MiB of headers)
quarters it. The CPU cost of binning was not measured, as nothing is built yet.

## Read

1. **Brute force is enough for a few dozen lights, not for hundreds.** On this GPU it passes the
   4 ms GPU budget up to about 64 lights at 4K (1.6 ms) and about 256 at 1080p (1.4 ms). At 256
   lights and 4K it costs 5.4 ms. The reference iGPU is untested and will be many times slower
   than this card, so its breakeven is far lower; it needs a measurement there before the
   threshold is chosen.
2. **Tiled or clustered lists are needed for hundreds of lights, and the cost is the loop.**
   A light that reaches nothing costs 88% of one that does, so a list that holds only the lights
   that touch a cluster takes the cost from lights in the level to lights at a pixel. At
   32 a cluster (the cap) it is at most about 0.6 ms at 4K (32 x 19.8 us), and a few lights
   a pixel is about 0.1 ms, against 5.4 ms. Use packed variable-length lists, not 32 fixed
   slots, and larger tiles at 4K.
3. **Where the cost is.** The face loop in `face.wgsl`: about 20 us a light at 4K, 5 us at 1080p.
   Voxels are not the problem at the vertex loop: 0.04 ms for 256 lights. If voxel lighting
   moves into the fragment shader to read clusters, it will take on the face loop's cost per
   pixel, which it does not pay today. Keeping voxels per vertex, with a short list a chunk
   (or the cluster at the vertex), keeps them cheap.
4. **A depth pre-pass is not worth it first.** Faces are drawn front to back and depth tested,
   and the rough check above shows about 1.5 shaded pixels per screen pixel (multisample edges
   included), so a pre-pass would save at most a third of the face loop, and costs a second
   geometry pass of all faces. Revisit if a real level, dense with overlapping faces, shows
   more. Clusters reduce the light loop for every shaded pixel including overdrawn ones.
5. **CPU is not the issue.** 0.09 to 0.26 ms for the frame at 256 lights, and 0.1 to 0.4 us to
   place a light on today's levels.

Limits: tiny levels, one GPU, the faces are large and few, no bump or specular cost
separated (High has both on), nothing run on the Intel part. Treat the per-light numbers as a
floor for a real level.
