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
it within one luxel row per worker; a bake that fails (`Atlas(TooTall)` on a very
large level) leaves the draft up, says so once and is not retried for that key.
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
