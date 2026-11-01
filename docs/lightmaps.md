# Lightmaps: what is baked, when, and where it is kept

All static lighting and all shadows come from one **baked atlas**. Nothing is
traced while playing: the frame loop only reads the atlas. A shadow therefore
cannot be regenerated, recomputed per frame or cached badly at run time; it is
a fixed set of numbers, the same on every frame and every launch.

## What a bake is

`mscent-world/src/lightmap/` projects every face onto a grid of luxels
(`BuildConfig` defaults: 0.25 units per luxel, lamp radius 0.3, 16 shadow
samples), traces shadow rays from each luxel to each lamp, adds one bounce, and
stores colour plus a dominant light direction per luxel. The result is one
`Lightmap`.

## When it runs

Once per distinct set of inputs. The cache key (`store::input_key`) hashes
exactly what the baker reads: face polygons and normals, lights, the bake
settings and the baker's `VERSION`. It does **not** include texture mappings,
materials, enemies, the player start, the camera or the render quality, so
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
projections, luxels, and a 64 bit checksum of everything before it. The reader
checks every count against the bytes left and every float for finiteness. A
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
cache directory. The build log states `Lightmap baked in ...` or `Lightmap
loaded from cache ...` or `Lightmap reused from the last bake (no bake ran)`.
Build refreshes the file, Save writes the current bake next to the saved level
(when the build has the document's geometry), and Instant Play (F5 on a current
build) plays the build that is on screen with no bake at all. A texture or
mapping edit keeps the lightmap; moving a brush or a light bakes again.

## Measured (this machine, release, showcase)

See the `a_warm_start_beats_a_cold_one_on_the_showcase` test output
(`cargo test --release -p mscent-render --test bake_cache -- --nocapture`) for
the cold bake against the warm load of the same level.
