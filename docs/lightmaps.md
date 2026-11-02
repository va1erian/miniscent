# Lightmaps: what is baked, when, and where it is kept

All static lighting and all shadows come from one **baked atlas**. Nothing is
traced while playing: the frame loop only reads the atlas. A shadow therefore
cannot be regenerated, recomputed per frame or cached badly at run time; it is
a fixed set of numbers, the same on every frame and every launch.

## What a bake is

`mscent-world/src/lightmap/` projects every face onto a grid of luxels
(`BuildConfig` defaults: 0.125 units per luxel, lamp radius 0.3, 64 shadow
samples), traces shadow rays from each luxel to each lamp, adds one bounce, and
stores colour plus a dominant light direction per luxel. The result is one
`Lightmap`.

## Voxel surfaces

Voxel volumes (`docs/voxels.md`) are lit by the same baker but not through the
atlas. Each surface cell, named by its place in its volume, has a record: colour
and the direction of the light, lit like a luxel from a point 1.5 cells off the
surface with the same soft shadows, from the same lamps and gain, so the two
kinds of surface read the same in the shader (colour times 256). Rays go through
`Occluders`: the BSP, then the triangles of every volume, so voxels shadow
brushes and brushes shadow voxels. A cell whose lifted point is inside solid brush
takes its light from the lit cells within two cells of it. Bounce uses one patch
per chunk and gathers per block of 4 cells, then blurs; brush luxels do not
gather from chunk patches. The records are stored in the `.msl.lit` file after
the luxels, and an old file (version 4) is ignored and rebaked.

## How big the atlas gets

The atlas is 1024 luxels wide and grows downwards to at most 8192 rows: 201 MiB
at 24 bytes per luxel, which stays under the 256 MiB a cache file may have.
`lightmap/layout.rs` lays the faces out at the luxel size asked for. A level past
that limit is laid out again with luxels 1.25 times larger, until it fits, before
any ray is traced. The whole level coarsens together, so no room looks sharper
than the next, and the bake report says so (`too big for the atlas: luxels
coarsened from 0.125 to ...`). A bake fails on the atlas only when one luxel per
face is too much, which takes about 900 000 faces.

Growing comes first because coarsening costs what the level looks like. Measured
on the 8 by 8 generated level (348 brushes, 1251 faces, 5.85 million luxels, 24
threads, release):

| settings | atlas | luxel size | bake | memory and file |
|---|---|---|---|---|
| grown limit (what ships) | 1024 x 6472 | 0.125 | 19.8 s | 152 MiB |
| old limit, coarsened | 1024 x 3032 | 0.244 | 10.1 s | 71 MiB |

The coarsened bake would have had half the detail along each axis, because the
limit is crossed in steps of 1.25 and 0.195 still overflows by a little. A wider
atlas holds no more for the same memory: this level takes 148 MiB at width 2048,
150 at 4096 and 155 at 8192, while the showcase goes from 11 MiB at 1024 to 13,
18 and 26 because a few tall shelves leave more waste. Keeping the width also
keeps the showcase's layout and every luxel of it.

The showcase is not affected: 1024 x 486, 11 MiB, about 140 ms before and after,
and every luxel is bit for bit the same. The generated level, which could not be
baked before, reloads from its file in 250 ms; the game's peak working set is 486
MiB after a cold bake and 314 MiB after a reload, and a frame takes 9.8 ms (`--screenshot`).

## When it runs

Once per distinct set of inputs. The cache key (`store::input_key`) hashes
exactly what the baker reads: face polygons and normals, lights, the shape of
voxel surfaces (positions, normals and triangles, `cell_sites::shape_key`), the
bake settings and the baker's `VERSION`. It does **not** include texture
mappings, materials (painting a voxel volume changes only its materials),
enemies, the player start, the camera or the render quality, so
changing any of those never re-bakes. Moving a brush or a light, changing the
settings, or a baker change (bump `store::VERSION`) does.

Never in the frame loop, on window resize, on a quality change or when leaving
and entering Play.

## Where it is kept

`<level>.msl.lit` next to the level (`showcase.msl` gives `showcase.msl.lit`).
Levels without a file (the built-in showcase and corridor, an unsaved editor
document) use the per-user cache directory, found without any dependency:
`MINISCENT_CACHE_DIR`, else `%LOCALAPPDATA%`, else `$XDG_CACHE_HOME`, else
`~/.cache`, followed by `miniscent/<name>.msl.lit`. The files are ignored by
git (`*.msl.lit`).

The format is little endian: magic, version, content key, atlas, per-face
projections, luxels, a count and the light records of voxel surface cells (9
bytes each: three `u16` colours in 1/2048 steps and three `i8` direction
components), and a 64 bit checksum of everything before it. The reader
checks every count against the bytes left and every float for finiteness. The
records are fixed width and any bytes are a valid record, so they need no range
check, but their count must use every byte that is left and equal the number of
surface cells of the level being loaded. A
missing, stale (key differs), old-version, truncated, bit-flipped or hostile
file is ignored, the reason is logged (`cache file ignored: stale: ...`), the
level is baked and the file is replaced. Files are written to a temporary name
and renamed, so a crash cannot leave a half-written cache.

## Game

```
miniscent                       # showcase; bakes once, then loads the cache
miniscent --level my.msl        # my.msl.lit next to it (or: miniscent my.msl)
miniscent --bake my.msl         # bake and write the cache only, print stats
miniscent --rebake              # ignore the cache, bake, overwrite it
miniscent --no-cache            # neither read nor write the cache
```

The start-up line says which happened: `lightmap baked in N ms (written to
PATH)` or `lightmap loaded from cache (N ms): PATH`. `python
tools/write_levels.py` rewrites `assets/levels/*.msl` and bakes their
`.msl.lit` files.

## Editor

Open sets the cache file next to the level; a new or unsaved document uses the
cache directory. Nobody asks for a bake: `apps/editor/src/studio/preview.rs`
keeps the 3D pane in step with the document, in two stages on their own threads.

A **preview** runs for every edit, one at a time, always on the newest document.
It compiles the level (`mscent_render::Compiled`) and uses the full lightmap if
memory or the file holds one with the same key; otherwise it bakes a **draft**
(luxels of at least 1 unit, hard shadows, no bounce). A texture or mapping edit,
or moving an enemy, finds the full lightmap and never shows a draft.

A **full bake** starts once the picture is current on a draft and the document
has been still for 150 ms. It uses all cores but one and holds no lock on the
cache, so previews, Save and Open carry on. A preview whose key differs cancels
it within one luxel row per worker; a bake that fails leaves the draft up, says
so once and is not retried for that key.
When a bake ends, the next preview finds the result and swaps it in.

The picture and the status bar say which stage it is in (`BAKING LIGHT`, `Baking
lighting...`, `Lighting up to date`), and the lighting log states `Lightmap
baked in ...` or `Lightmap loaded from cache ...`. Save writes the current bake
next to the saved level when the picture is fully lit with the document's
geometry; a bake still to come writes there by itself. Instant Play (F5) needs
only the preview: a full lightmap that finishes during the game replaces the
draft under it and the game carries on.

A cache file is checked by its header first, so a stale one costs one small read
per preview instead of a read of the whole atlas.

## Measured (this machine, release, showcase)

See the `a_warm_start_beats_a_cold_one_on_the_showcase` test output
(`cargo test --release -p mscent-render --test bake_cache -- --nocapture`) for
the cold bake against the warm load of the same level.
