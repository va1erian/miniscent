# Texture Studio: plan

Status: built. M12.1 to M12.11 are done, and the workspace is on the Texture menu.
The sections after "As built" are the plan as written, kept for its reasoning;
"As built" overrides them. Work items are in [work-items.md](work-items.md). To learn
the tool, follow the [tutorial](texture-studio-tutorial.md).

## As built

- **All stock materials ship as recipes.** The ten stock materials are
  `assets/textures/*.mst`, 11.4 KB against 902 KB of PNGs, and the game makes them
  at load. `tools/gen_textures.py` is gone. So `mscent-render` depends on
  `mscent-texgen`, unlike the plan below, and every caller of `load_dir` gets
  recipes without knowing. PNG materials still load, and a recipe wins over a PNG
  of the same name.
- **Measured (`texgen_bench`, size-optimized release, dev machine).** All ten stock
  recipes take 42.8 ms at 128 px, and one parameter change on brick at 256 px takes
  19 ms. The game binary grows by about 140 KB to 2.02 MB, 92% of its 2.2 MB budget.
- **Lookalikes.** Recipes reproduce the old pictures closely, not exactly, since
  numpy's FFT noise is not reproducible in Rust. Metal and floor gained specular
  masks, which closes M5.7. Detail finer than a texel raised shimmer past its limit
  until recipes stopped their octaves where the old generator did: at 128 px, a
  frequency doubled past 64 adds only aliasing.
- **Save** writes the recipe and `materials.txt`, never PNGs, and
  `--bake-texture` writes PNGs to look at and adds a row when the directory it
  writes to has a manifest.
- **`--bake-texture` checks first.** It reads the recipe up to `MAX_FILE`, and
  refuses an invalid name or a `name.mst` already in the output directory (which
  the game would load in place of the PNGs) before it writes anything.
- **The workspace and Play.** Opening the workspace cancels an F5 still waiting for
  the level, and Play started by any route closes the workspace first. Esc during a
  drag of a block cancels the drag, and a second Esc goes back to the level; an
  undo, a deletion or another material during a drag also ends it with no move.
  "Working..." in the preview ends when the work does, including a page that does
  not compile or whose first draft fails.
- **Stacked blocks, no wires.** A block reads the row directly above it, under its
  columns, left to right. A generator below a block starts a new chain.
- **Preview.** Six tabs. Block shows the selected block; the others show one map
  of the finished material, whichever block is selected. The tab stays chosen
  when the selection moves, so one map can be watched while any block is edited.
- **Evaluation.** Every request runs a draft of at most 128 px that keeps all
  blocks, for the thumbnails, then the full size keeping only the selected block
  and the outputs, unless a drag is going on. A page of 256 blocks kept at 1024 px
  would not fit the evaluator's memory limit.
- **Save also applies.** The running table and the disk must give a material
  the same id, because levels store ids. Before writing, Save works out the id
  the material has in the running table and reads the manifest again from the
  disk; if the disk's id differs it refuses and names the materials to save
  first, so applying A and then saving B is refused until A is saved. It also
  refuses a new name whose `.mst` file is already there unlisted, and so does
  New. Saving over any of the ten stock materials asks first (the list is the
  studio's own, held to the shipped `materials.txt` by a test). A read error on
  the old recipe stops the save, since only a missing file means there was none to
  put back, and files are synced before they are renamed into place.
- **Save proves the page the way the game runs it.** Before any file is touched,
  the worker reads back the text it will write, then compiles, runs (no cache) and
  packs that, which is what `mscent_texgen::bake` does, with a cancel flag so
  closing the editor does not wait for a slow bake. The texture it installs is that
  one. A row whose recipe fails would stop the game loading any material.
- **Applied, not saved.** A material applied under a new id is in the running table
  only. Saving a level that uses one still saves, and the status bar names it in a
  warning, since next session those faces would draw grey or another material.
  Only brush faces are checked, not the materials of voxel volumes.
- **Opening reads on the worker.** A recipe file is read and parsed on the worker
  and the material opens on a later tick, so a large file never holds the UI
  thread; the material on show stays until then, and one open at a time.
- **Lit view.** `mscent_render::texel_light` holds the steps the lighting shader
  takes for a pixel once it knows the light, inlined into the shader, and
  `shade_texel` is those steps in one call. The tile is drawn at most 512 px wide.
- **The page shows what a block reads.** Each input a block actually reads, from
  the compiled graph, is a small mark on its top edge, and the parameter panel
  says it in words ("reads 1 input: noise", or the name a `load` reads). The marks
  are the compiled graph's, so a page with an error shows none and the error text
  says why. The page scrolls by the wheel, a middle-button drag or by selecting a
  block, which brings it into view; a bar along the edge shows where the window
  is on a longer page.
- **Paint.** With a `paint` block selected and the Block tab showing, a drag on the
  preview draws a stroke. Positions are in tiles, the 2x tiled view included, and
  a stroke is kept as one continuous run of coordinates, so one that crosses an edge
  continues on the other side. A stroke is one undo step, and its points are thinned
  to a quarter of the brush radius apart (at least 1/1024 tile) so a long one stays
  small, and every point is rounded to 1/4096 of a tile as it is added. It stops
  growing at `STROKE_POINTS` (2180) with a message, and a block past `MAX_STROKES`
  refuses a new one. `STROKE_POINTS` is under the operator's `MAX_POINTS` (4096)
  because a stroke is one line of the file and the reader refuses lines past 64 KB:
  4096 points take up to 30 bytes each however they are rounded. The recipe's undo
  history is kept to 256 steps and an estimated 64 MB, the oldest going first, since
  a stroke list is copied whole into each step. While the pointer is down the worker runs the
  draft path, as for a slider, and the full picture comes on release. The brush
  (radius, hardness, colour) belongs to the workspace, so it is no undo step. The
  recipe reader holds `stroke` lines to `mscent_texgen::ops::charge_stroke`, the rule
  the kernel applies, so a file that reads also runs, at any size.
- **Limits on work.** The game makes recipes at startup on one thread with no way to
  cancel, so a small file must not be able to stall it. Each operator has a cost
  (`OpSpec::cost`, nanoseconds per texel on its worst parameters, or worked out from
  octaves, inputs or strokes), measured by the `texgen_costs` example and rounded up.
  `graph::run`, and so `bake` and `load_recipe`, refuse with `TooLarge` before any
  pixel when a page's kernels at that size add up to more than `MAX_RECIPE_NS` (5 s).
  `MaterialTable::load_dir` adds up its recipes, with their packing and reading, and
  refuses past `MAX_DIRECTORY_NS` (30 s), also before any pixel; the stock directory
  declares 47 ms. PNGs are not counted. Costs depend only on the recipe and the size,
  never a clock, so a page gets the same answer on every machine; thirty `dots`, the
  dearest ordinary block, still fit at 1024 px.
- **Limit on strokes.** `paint`'s rule is one function, `ops::charge_stroke`: for each
  segment `(length + 2r) * 2r` in tile units, with `r` the radius plus two texels of a
  1024 px tile, plus 1/65536 per point, up to `MAX_STROKE_WORK` (360). It does not
  know the tile's size, so strokes that work at 1024 px work at 128 px. At a brush of
  radius 0.2 thinned as the editor thins it, 360 is 100 tiles of drawing. The
  rasterizer walks each segment's rows and spans, not its bounding box, so a long
  thin diagonal costs what the rule says; the pictures are the same to the bit.
- **Limit on memory.** `graph::peak_images` plans the peak from the graph before any
  kernel runs: the run's order and readers, with each kernel block counted as its own
  image whatever a cache would share. Over `MAX_LIVE_BYTES` (512 MiB) is `TooLarge`, so
  the answer is the same in the editor, with a cache, and in a bake, with none.
- **Measured worst cases** (`texgen_costs` and `recipe_limits`, release profile, best
  of three, 1024 px, dev machine with other work running). Before, 254 `paint` blocks
  took 233.9 s, 254 full `scratches` 87.3 s and `dots` 33.7 s. Those pages are now
  refused in under 0.3 ms, and the most of each that passes takes: 4 such `paint`s 2.9 s;
  26 `scratches` 3.9 s; 31 `dots` 4.1 s; 39 `blur` 4.5 s; 34 `sharpen` 3.3 s; 39 `noise`
  of 8 octaves 4.7 s; 33 `cavity` 3.8 s. No page of any operator passed at over about
  5 s. One `paint` at the most the rule takes, on the costliest shape, takes 3 s.
  Reading is linear, at most 5 ns a byte: a 4 MiB file takes 14 ms, not 12.6 s. A
  directory at its limit, 6 rows of 33 `cavity` blocks, takes 28.1 s, and 7 rows are
  refused in 5 ms. Running slower than this on another machine, or under load, is
  possible: the limits are in estimated work, not seconds.
- **A material with no recipe** (PNG files) opens on a new page of its size, and
  nothing replaces the files until it is saved, which asks first for a stock one.
- **Bump fade at 512 px.** A tile's world size is `texture_size * scale`, so at the
  default scale a 512 px map has the texel density of a 128 px one and fades at
  the same distance. Only a 512 px map scaled to a 128 px map's tile has 4 times
  the texels per pixel, and it reaches `BUMP_GONE` at a quarter of the distance.
  The lit view draws one texel per pixel and does not show this. This is worked
  out from the shader's constants; no picture of it was taken.

## Goal

Move what `tools/gen_textures.py` does into the editor, as a tool anyone can
use without Python or numpy. A material is a small **recipe**: a graph of noise,
drawing, filter and combine operators. The recipe produces albedo, height and
specular, and the normal map is derived from the height. Changes show at once
on a lit tile and, when applied, in the 3D view.

Not in scope: PBR maps beyond these, GPU evaluation, importing photos, wired
node graphs, animated textures, and custom emissive materials (see "Lit view").

## What exists today

| Fact | Where | What it means here |
|---|---|---|
| Five textures come from Python: fractal noise through FFT upsampling, plus masks built by hand per material | `tools/gen_textures.py` (since removed) | The operators below must be able to rebuild all five; that is the test of the op set |
| Normals: wrapped central differences per texel, `(-dx, dy, 1)` | `to_normal` in the same file | Keep this sign convention, or every bump flips. Per-texel differences also mean relief changes with resolution; see "Resolution" |
| Height is 8-bit on disk, and normals are computed before quantizing | same | The generator keeps more bits so gentle slopes do not stair-step |
| Material ids are positions in the sorted file list, and levels store the id | [materials.rs](../crates/mscent-world/src/materials.rs), `check_materials` in [level/mod.rs](../crates/mscent-render/src/level/mod.rs) | Adding `concrete.png` shifts `floor` and later ids, and the level no longer builds. **This blocks custom materials** |
| Gloss is a hard-coded `match` on the name, and `_s.png` is skipped on load (M5.7 not done) | `default_gloss`, `load_dir` in [texture.rs](../crates/mscent-render/src/texture.rs) | Specular maps need engine work, not just export |
| The editor loads materials twice: once in a `OnceLock` for previews, once for the face panel's thumbnails and `texture_sizes`, each a single time | `materials()` in [build.rs](../apps/editor/src/build.rs), `load_previews` in [facepanel.rs](../apps/editor/src/ui/facepanel.rs) | Blocks live updates, and all three consumers must follow a reload. The `OnceLock` comment says a clone shares the pixels, but `Texture` owns `Vec`s, so each clone copies them |
| Every renderer filters its whole table and rebuilds mips | `assemble` in level/mod.rs | Shared materials need filtering once, at table build |
| The brush panel's material list is the fixed `NAMES`, and xui's `ComboBox` has no way to replace items | [props.rs](../apps/editor/src/ui/props.rs) | A brush with id 5 or higher shows the wrong name. The list and the thumbnails must be remounted on a reload |
| A texture's world size is `texture_size * scale`, with scale and shift per face in texels | [texinfo.rs](../crates/mscent-world/src/texinfo.rs) | Changing a saved material's size rescales it in every level; see "Resolution" |
| Bump fades by texels per pixel | `BUMP_FULL`, `BUMP_GONE` in [lighting_shader.rs](../crates/mscent-render/src/lighting_shader.rs) | A larger normal map loses its relief closer to the camera |
| `lightpanel` is emissive by id, drawn flat with no bump or specular | `face.material == LIGHTPANEL` in [fill.rs](../crates/mscent-render/src/level/fill.rs) | Its normal, gloss and mask have no effect in the game, and new materials cannot be emissive |
| The lightmap key hashes geometry, lights and bake config, and bounce uses one configured albedo | [store.rs](../crates/mscent-world/src/lightmap/store.rs), [bounce.rs](../crates/mscent-world/src/lightmap/bounce.rs) | Editing a texture needs no re-bake |
| xui at the pinned rev has `Tabs`, `Slider`, `ColorPicker`, `ComboBox`, `TreeView`, `Split` and context menus; the editor already has custom-painted nodes and hideable `stack`s | [app.rs](../apps/editor/src/ui/app.rs), [view3d.rs](../apps/editor/src/ui/view3d.rs) | No xui changes or new dependencies expected |
| Normal texels are `0xAARRGGBB`, and decoding ignores alpha | `Texture::normal`, `NormalView` in texture.rs, [bump.rs](../crates/mscent-render/src/bump.rs) | The specular mask can live there; see "Engine changes" |
| `encode_png` writes 8-bit RGB with stored (uncompressed) deflate | [png_encode.rs](../crates/mscent-render/src/png_encode.rs) | It loads correctly, but a 128 px map becomes about 49 KB (today 305 bytes to 29 KB) and a 1024 px map about 3 MB |
| The game binary has a size budget (2.2 MB) | [check_size.py](../tools/check_size.py) | The game links the generator only if that is measured and worth it |

## What to take from kkrieger

.kkrieger shipped operators and parameters instead of pixels, and rebuilt its
textures at load. The public source of the same tool family is
[werkkzeug3's `genbitmap`](https://github.com/farbrausch/fr_public/blob/master/werkkzeug3/genbitmap.hpp).
It has operators such as `Flat`, `Perlin`, `Cell`, `Gradient`, `GlowRect`,
`Dots`, `Bricks`, `Text`, `Merge`, `Mask`, `Paste`, `Color`, `Range`, `HSCB`,
`ColorBalance`, `Blur`, `Sharpen`, `Rotate`, `Twirl`, `Distort`, `Bulge`,
`Normals`, `Light` and `Bump`, on 64-bit pixels whose channels are effectively
15 bits (`GetColor64` masks with `0x7fff`). The page UI, the store and load
operators and reference-count freeing below are from memory of the tool, not
confirmed from source; M12.3 checks them first.

| Idea | Adopted as |
|---|---|
| One pixel format for every operator | RGBA at 16 bits per channel, so any output feeds any input. Grey data has R = G = B |
| Parameters, never pixels | Recipes are text files. PNGs are a build product |
| Operators stacked on a page, where the input is the block above | The graph editor: no wires to route |
| Named store and load | How one mask drives both albedo and height, as `mortar` does in `brick()` |
| Intermediate results freed by reference count | The evaluator, plus a cache for interactive edits |

Different on purpose: hand drawing is added, stored as replayable strokes.
A `bump` operator bakes height directly into albedo, where `Bitmap_Bump` reads a
normal map and only `Bitmap_Normals` reads height. Coordinates are normalized,
and everything tiles, with the rules below.

## Architecture

```
crates/mscent-texgen     no dependency except mscent-math (Rng)
  image    Rgba16 buffers, wrapped sampling, iterative box blur
  ops/     pure kernels: (params, inputs, size) -> image
  graph    pages -> checked DAG, evaluation order, refcounts, memory cap
  cache    results keyed by hash(op, params, input hashes, size)
  recipe   versioned text format, bounds-checked reader and writer
  pack     outputs -> albedo, height, normal, specular as packed texels

apps/editor
  model/texdoc.rs   the recipe being edited, with its own undo stack
  texstudio/        evaluation worker, cancel, draft then full resolution
  ui/texstudio/     workspace, op page, parameters, previews, paint tool
```

- **Edges.** The new crate depends on `mscent-math` only, and the renderer does
  not depend on it. The editor converts its output into `Texture`. `AGENTS.md`
  records the new edge.
- **Naming.** `Studio` already names the preview jobs in the editor, so the code
  uses `texstudio`.
- **Separate document.** `Document` equality drives CSG and preview jobs, so
  recipes stay out of it. Dragging a slider starts no level work.
- **Determinism.** The same recipe should give the same bytes on every platform.
  Kernels use `+ - * /` and `sqrt`; any `sin`, `cos` or `pow` comes from tables
  built once by an in-repo series. All randomness is a seeded `mscent_math` Rng
  per operator, so changing one seed never disturbs another operator. CI checks
  a golden hash per operator from M12.2 on. CI runs on Linux and development on
  Windows, so that covers two platforms; LazyOS is unverified until there is a
  port.
- **Hostile input.** Recipes are untrusted, like levels. Caps: 256 operators,
  1024 px per side and powers of two only, 4096 strokes per paint operator,
  bounded octaves, cells and dots, and a memory limit checked before allocation.
  Evaluation is iterative, not recursive. Errors are returned, never panics.

### Resolution

- **Patterns** are in normalized coordinates, so recipes evaluate at any size.
- **Relief.** `normal_strength` is defined at 128 px and multiplied by
  `size / 128`, which matches `to_normal` at 128 px and keeps draft and full
  relief alike. A test compares them.
- **Saved size.** A material's size sets its world scale, so a new material picks
  its size at creation. Changing the size of a saved material warns that it
  rescales the texture on every face using it. The panel shows the world tile
  size at the default scale. A logical size independent of texels would remove
  this limit, but changes `uv_at`, and is left for later.
- **Bump fade** stays per texel, so large maps fade sooner. The lit view shows
  this only at its zoom, so M12.7 records the visible difference at 512 px.

### Tiling

Sampling with wrap is not enough. Each operator has one of these rules, and the
parameter table enforces it:

| Operators | Rule |
|---|---|
| `noise`, `cells`, `wavelet` | Periodic lattice with integer frequency; 0 is legal per axis, for brushed metal |
| `bricks`, `checker` | Reject odd counts where the pattern alternates (`offset` with odd rows, `checker` with odd counts) |
| `stripes` | Integer cycles on x and y, anti-aliased, for diagonal chevrons |
| `gradient` | Triangle wave, so it returns to its start |
| `glow`, `rect`, `ellipse`, `line`, `dots`, `paint` | Wrapped distance |
| `blur`, `sharpen`, `distort`, `merge`, `mask`, `color`, `range` and other per-pixel filters | Keep the period of their inputs |
| `rotozoom` | Integer lattice only |
| `twirl`, `bulge` | Radius capped at 0.5 tiles, so the warp is zero before the edge |

The seam test evaluates the operator on a tile and on the tile shifted by half
its size, then requires the two to agree after shifting back. Unlike an
edge-difference metric, this does not trip over a mask edge sitting on x = 0.

### Recipe format

Text, diffable, in the spirit of `level_text`. Layout is the graph: a block reads
what sits directly above it, in its columns from left to right. Each input
column must hold the same block or start a new one, and a block with too few
distinct inputs above it is an error. Blocks can only read upward, so cycles can
only come through `store` and `load`, and the checker rejects those.

```
miniscent-recipe 1
name mottled
size 128 128
gloss 25
# x y w op
op 0 0 1 noise freq=4 octaves=5 persistence=0.55 seed=1
op 1 0 1 flat  color=0.55,0.22,0.15
op 0 1 2 merge mode=multiply
op 0 2 1 out-albedo
op 1 2 1 noise freq=8 octaves=4 persistence=0.5 seed=2
op 1 3 1 out-height normal_strength=5
```

`gloss` seeds the material when it is first saved; Save writes it to
`materials.txt`, which is what the game reads.

### Operators

| Group | First set (M12.2) | Later set (M12.9) |
|---|---|---|
| Noise and patterns | `flat`, `noise` (fbm, ridged), `bricks` (mask, per-brick tint and bevel modes), `checker`, `stripes`, `gradient`, `glow` | `cells` (Voronoi), `wavelet` |
| Drawing | `rect`, `ellipse`, `line`, `dots` (grid or scattered stamps, for rivets), `paint` (M12.8) | `text` with the in-repo bitmap font, `scratches` |
| Filters | `color` (gain may exceed 1, as `brick()` does), `range`, `invert`, `hscb`, `blur` | `sharpen`, `rotozoom`, `twirl`, `bulge`, `distort` |
| Combine | `merge` (add, subtract, multiply, min, max, screen, blend), `mask` (three inputs: a, b, mask), `paste` | |
| Height tools | | `bump` (bake light into albedo), `cavity`, `slope` (specular from height) |
| Flow | `store`, `load`, `out-albedo`, `out-height`, `out-specular` | |

A block reads the blocks above it, so a position is a connection and there are no
wires to show it. An operator whose input is optional (`rect`, `dots`, `paint` and
the other shapes) therefore draws over the block above it if there is one, and on
its own background only if there is none. The page marks every input a block reads
on its top edge, and the parameter panel says how many and from which blocks, so a
shape placed under another block is seen to read it.

A recipe needs `out-albedo`. Without `out-height` the normal map is flat, and
without `out-specular` the mask is full. Height and specular read channel R.
All five stock recipes need only the first set.

## Engine changes

1. **Stable ids.** A new `assets/textures/materials.txt` lists `id name gloss`,
   and it decides the ids. Stock ids 0 to 4 do not change and cannot be retired,
   since `check_materials` needs them. Ids are append-only: deleting a material
   marks its row retired, which keeps a placeholder slot that draws the grey
   fallback, so existing levels keep working. A directory with no manifest keeps
   sorted ids, with the old gloss values for stock names and 40 for others, so
   existing `--assets` directories render the same. A PNG not listed is ignored,
   and a row with no PNG is an error. Names become file paths, so they match
   `[a-z0-9-]`, have a length bound, and exclude Windows reserved names.
2. **Specular map (engine side of M5.7).** On load, `_s.png` goes into the alpha
   of the normal texels, which are already fetched per pixel, so there is no
   second texture fetch. The highlight is multiplied by that alpha, only in the
   `SPEC = true` shader. Without `_s` the load forces alpha to 255, including for
   an RGBA `_n.png`, so existing materials must render the same pixels; a test
   asserts it. The mask fades to the material's mean with `bump_scale`, because
   the normal array has no mips and an unfiltered mask would shimmer. A `_s.png`
   with no `_n.png` is a load error, since the mask has nowhere to live; Save
   always writes a normal map. M12.5 also handles the paths that skip the fetch
   or the highlight: zero `bump_scale`, the `is_visible` fallback and the emissive
   flat path.
3. **Reloadable table.** `MaterialTable` holds `Arc<Material>`, filtered once at
   table build, and `assemble` skips textures already filtered. The editor keeps
   one table plus a generation number in place of the two loads. A new generation
   updates the preview through the existing preview path and keeps the lightmap.
4. **Dynamic material lists.** The brush panel's combo, the face thumbnails and
   `texture_sizes` follow the generation. The combo and thumbnails are remounted,
   each in its own container, since xui cannot replace their items.

`load_materials` also returns the directory it used, which is where Save writes.

## Editor workspace

A new child of the outer `stack` in `app.rs` (the one holding the work area and
Play), since only that one hides the palette and side panel. Opened from a new
Texture menu. The level document is untouched. While it is open, the global
shortcuts route undo and redo to the recipe's history.

```
+-- materials --+------------ preview ------------+-- parameters ---+
| brick         | [Albedo|Height|Normal|Spec|Lit]  | bricks          |
| floor         |  [1x|2x tiled] [draft|full]       | rows    [8    ] |
| metal         |                                   | cols    [4    ] |
| + New         |      picture of selected op       | offset  [--o--] |
|               |                                   | seed    [7] [?] |
|               +-----------------------------------+                 |
|               | op page: blocks on a grid         | brush (paint)   |
| Save          | [bricks][noise ]                  |                 |
| Apply         | [store ][store ]                  |                 |
+---------------+-----------------------------------+-----------------+
```

- **Op page.** A custom-painted node: click selects, drag moves, a right-click
  menu adds. Each block has a small thumbnail. The preview follows the selected
  block, so any stage of the chain can be inspected.
- **Parameters.** Generated from a table per operator (name, range, kind), so an
  operator needs no UI code of its own. Edits are undoable.
- **Evaluation.** Runs on a worker thread with cancel, as `build.rs` jobs do,
  and only re-runs what the cache misses. Drafts at 128 px show during a drag,
  then full size. The UI tick never waits.
- **Lit view.** A tile lit by an orbiting light. The game's final colour also
  goes through private code (`mul_rgb3`, `add_spec`, `soft_limit`, `SPEC_GAIN`),
  so `mscent-render` exposes one small per-texel shade function that both use,
  and a test compares it with the game's shader on a synthetic face. Emissive
  stays hard-coded to `lightpanel`, and the view draws that one flat, as the game
  does.
- **Paint.** Drawing on the preview adds strokes to the selected `paint`
  operator: normalized points, radius, hardness, value. Strokes wrap across the
  edge, and the 2x tiled view makes that visible. One stroke is one undo step.
- **Save and Apply.** Save writes the recipe, the PNGs and `materials.txt` by
  temp file and rename, and asks before overwriting a stock material. Apply
  installs the material in the running editor, where it joins the face browser.

## Work items

Each leaves the game and the editor working. Sizes as in work-items.md.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M12.1 | `mscent-texgen`: `Rgba16`, wrapped sampling, tables, periodic noise; `texgen_bench` example; entries in both Cargo profiles | | M | Half-shift seam test for every frequency; same bytes twice; golden hash in CI; noise at 128 px timed |
| M12.2 | First operator set and the parameter table | M12.1 | L | Per-operator tests read real pixels; seam test on every operator and on chains; golden hash per operator; invalid parity and hostile parameters are errors, not panics |
| M12.3 | Confirm the page, store and load model against the Werkkzeug source, then pages to DAG, evaluation, refcounts, cache, recipe text format | M12.2 | L | Round trip; bit-flip and truncation fuzz; `store`/`load` cycles and misuse rejected; memory limit enforced before allocating |
| M12.4 | Output pack and headless bake: `mscent-editor --bake-texture <recipe> <dir>` | M12.3 | M | The normal of a test height at 128 px is within 1 level of `to_normal`; draft and full relief agree; PNGs load through `load_texture` |
| M12.5 | Engine: all of "Engine changes" | | L | Adding a material leaves stock ids and existing-level pixels unchanged; forged manifests return errors; a masked highlight differs by pixel test; a new generation reaches the lists and the 3D view; `check_frame_cost.py` and `crawl`/`stability` before and after |
| M12.6 | Workspace shell: list, op page, parameters, preview, undo, worker | M12.3 | L | `--selftest` builds a recipe through messages and reads preview pixels; `tick_costs` `assert_cheap` during a drag, and a clock-free test that the tick never waits for the worker, as `cache_lock_tests.rs` does |
| M12.7 | `--bake-texture` updates the manifest; lit view, Save and Apply, reload into the 3D view | M12.4, M12.5, M12.6 | M | Selftest applies a change and the 3D view's pixels change while the lightmap key does not; lit view matches the game's shader on a synthetic face; the saved material loads in the game; an agent can make a usable material with no window |
| M12.8 | Paint operator and tool | M12.6 | M | Replay at 128 and 512 px, downsampled, is within 2 levels mean; a stroke across the edge passes the seam test; undo restores identical pixels |
| M12.9 | Later operator set, starter templates | M12.3, M12.4 | L | Same tests as M12.2; each template bakes into a contact sheet |
| M12.10 | Stock recipes for all five materials with `out-specular` for `metal` and `floor` (closes M5.7), retire `gen_textures.py`; a compressing PNG encoder if file size needs it | M12.7 | M | Side-by-side contact sheet reviewed by the owner; full test suite passes, with snapshot changes explained; CI checks that committed PNGs equal the recipe output; assets size recorded |
| M12.11 | Game builds materials from recipes at load | M12.10 | M | Decided on measured numbers: binary size against the budget, load time against the 67 ms PNG decode noted in `build.rs` |

M12.5 is independent of the rest and can start at any time, so it can run in
parallel with M12.1 to M12.4. A first usable release is M12.1 to M12.7.

Targets to check against `texgen_bench`: all five stock recipes at 128 px in
under 100 ms total on the dev machine, and one parameter change at 256 px under
50 ms. Numbers on the i3-7100U go in `docs/perf/`.

## Risks

- **Stock materials shift.** Rust noise cannot reproduce numpy's FFT noise bit
  for bit, so M12.10 gives lookalikes, and snapshot tests against stock textures
  will change. PNGs stay as they are until the owner accepts the new ones.
- **Specular cost.** No fetch is added, but weighting a fourth channel may need a
  `view_normal` variant, and High is the default quality. If
  `check_frame_cost.py` objects, the mask becomes per-material again.
- **Levels hold ids only.** A level using a custom material needs the matching
  `materials.txt`. A missing id draws the grey fallback and does not fail. Saving
  material names in levels would be a format version 5 and is left for later.
- **Page editing.** Decided: stacked blocks, not wired nodes. They are cheap and
  compact, but unfamiliar next to wired graphs, so M12.6 should be tried early.
  Recipes are a DAG underneath, so another front end would not change M12.1 to
  M12.5.
- **Release profile.** The workspace builds for size. The new crate needs
  `opt-level = 3` as the renderer has, and that matters to the game's size only
  if M12.11 happens.
