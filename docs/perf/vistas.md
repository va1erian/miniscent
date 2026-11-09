# What a vista costs a frame

`cargo run --release -p mscent-render --example vista_bench`: the Monument demo
(`assets/levels/facing.msl`) at 640x360. Variants take turns batch by batch, and the fastest
batch of each is reported. Each row is the range of three runs on a quiet dev machine with
24 hardware threads, not the i3-7100U.

- `wall`: the sky faces are plain metal.
- `holes`: viewport faces with no layers, so only the mark, the resolve and the gradient.
- `vista`: the level as shipped.

Milliseconds per frame on one worker:

| Pose | Quality | wall | holes | vista | vista / wall |
|---|---|---|---|---|---|
| At the gate | High | 7.18 to 7.34 | 3.23 to 3.25 | 10.08 to 10.32 | 1.4 |
| At the gate | Low | 4.77 to 4.84 | 2.29 to 2.35 | 8.95 to 9.14 | 1.9 |
| Sky only | High | 6.83 to 7.02 | 0.83 to 0.86 | 6.75 to 7.04 | 1.0 |
| Sky only | Low | 4.25 to 4.58 | 0.85 to 0.92 | 6.56 to 7.15 | 1.5 |

On every hardware thread, with the game's bands of 24 rows:

| Pose | Quality | wall | holes | vista |
|---|---|---|---|---|
| At the gate | High | 1.17 to 1.31 | 1.09 to 1.22 | 1.57 to 2.05 |
| At the gate | Low | 0.66 to 0.92 | 0.52 to 0.54 | 1.42 to 1.92 |
| Sky only | High | 0.86 to 0.91 | 0.21 to 0.29 | 1.43 to 1.60 |
| Sky only | Low | 0.59 to 0.79 | 0.20 to 0.22 | 1.40 to 1.68 |

At the gate the frame shows 393 050 pixels, 275 107 of them from the vista, with 78 vista faces
and 13 168 vista triangles. Looking up it shows 367 106, 326 378, 77 and 8458. Vista pixels
overlap, which is why a frame draws more pixels than the 230 400 on screen.

The first demo had one-unit cells and 70 253 vista triangles at the gate, which cost 15.8 to
22.6 ms at High on one worker. Four-unit cells took it to 13 168. No pose of a sweep over the
platform (6 positions, 36 yaws, 4 pitches) passes 17 162, against a `VISTA_VOXEL_TRIS` of 24 000.

M13.11 replaced the three ranges with one range for the whole ground. `vista_bench --sweep`
measures the sweep now: at most 15 516 triangles for the three ranges and 22 790 for the
ground, and 16 572 at the gate, which costs 15.1 ms on one worker against 15.3 for the three
ranges in the same run. The tables above are older and from a quieter machine. With the
road narrowed to 55 and seed 7 (M13.12) the sweep's most is 21 764, the gate 14 446 at
9.5 ms.

## Levels with no vista

`frame_bench --workers 1 --json`, alternating with a build of `main` five times:

| Build | ms per frame | Ratio to calibration |
|---|---|---|
| `main` (97bc7a7) | 1.383 to 1.392 | 8.48 to 8.66 |
| This work | 1.342 to 1.358 | 8.28 to 8.42 |

No slowdown. This work reads about 3 % faster, and nothing in it was meant to speed that frame,
so it is taken as noise or code layout, not a gain. `tools/check_frame_cost.py` fails on this
machine on both builds, against a baseline of 7.61.

## Light

`cargo run --release -p mscent-render --example scale_lightmap`: the same apparent range,
256 across, at scale 1 and at scale 16.

| Scale | Luxel factor | Faces | Atlas luxels | Cell records | Chunks | Bake |
|---|---|---|---|---|---|---|
| 1 | 8 | 22 | 70 688 | 100 477 | 1280 | 754 ms |
| 16 | 1 | 22 | 63 120 | 26 146 | 192 | 197 ms |

Atlas luxels are close because both rooms' big faces reach the cap of 128 a side. 70 688 is
0.84 % of the 8.4 M atlas.

## Load

`texgen_bench`: stars 50 ms, clouds 40, ridge 44, planet 35, about 170 ms together, against 41 ms
for the ten older recipes. Levels pay this whether or not they use a sky, because the game makes
every stock material at load. The game binary is 2,120,192 bytes, 96 % of the 2.2 MB budget.
`facing.msl` is 380 010 bytes, down from 4.8 MB.
