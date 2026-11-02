# Texture Studio: plan

Status: draft, nothing here is implemented. Time and size figures are targets
to measure, not results. The work items below move to
[work-items.md](work-items.md) as M12 once this is accepted.

## Goal

Move what `tools/gen_textures.py` does into the editor, as a tool anyone can
use without Python or numpy. A material is a small **recipe**: a graph of noise,
drawing, filter and combine operators. The recipe produces albedo, height and
specular, and the normal map is derived from the height. Changes show at once
on a lit tile and, when applied, in the 3D view.

Not in scope: PBR maps beyond these, GPU evaluation, importing photos, wired
node graphs, and animated textures.

## What exists today

| Fact | Where | What it means here |
|---|---|---|
| Five textures come from Python: fractal noise through FFT upsampling, plus masks built by hand per material | [gen_textures.py](../tools/gen_textures.py) | The operators below must be able to rebuild all five; that is the test of the op set |
| Normals: wrapped central differences, `(-dx, dy, 1)` | `to_normal` in the same file | Keep this convention exactly, or every bump flips |
| Height is 8-bit on disk, and normals are computed before quantizing | same | The generator works at 16 bits so gentle slopes do not stair-step |
| Material ids are positions in the sorted file list, and levels store the id | [materials.rs](../crates/mscent-world/src/materials.rs), `check_materials` in [level/mod.rs](../crates/mscent-render/src/level/mod.rs) | Adding `concrete.png` shifts `floor` and later ids, and the level no longer builds. **This blocks custom materials** |
| Gloss is a hard-coded `match` on the name, and `_s.png` is skipped on load (M5.7 not done) | `default_gloss`, `load_dir` in [texture.rs](../crates/mscent-render/src/texture.rs) | Specular maps need engine work, not just export |
| The editor loads materials once per process, in a `OnceLock` | `materials()` in [build.rs](../apps/editor/src/build.rs) | Blocks live updates. Its comment says a clone shares the pixels, but `Texture` owns `Vec`s, so each clone copies them: cheap at 128 px, not at 1024 |
| The brush panel's material list is the fixed `NAMES`; the face panel reads the table | [props.rs](../apps/editor/src/ui/props.rs), [facepanel.rs](../apps/editor/src/ui/facepanel.rs) | A brush with id 5 or higher would show the wrong name |
| Bounce light uses one configured albedo, not texture colours | [bounce.rs](../crates/mscent-world/src/lightmap/bounce.rs) | Editing a texture needs no re-bake |
| xui at the pinned rev has `Tabs`, `Slider`, `ColorPicker`, `ComboBox`, `TreeView` and `Split`; the editor already has custom-painted nodes and a `stack` of hideable arrangements | [layouts.rs](../apps/editor/src/ui/layouts.rs), [view3d.rs](../apps/editor/src/ui/view3d.rs) | No xui changes or new dependencies expected |
| Normal texels are `0xAARRGGBB`, and decoding ignores alpha | `Texture::normal`, `NormalView` in texture.rs | The specular mask can live there; see "Engine changes" |
| The game binary has a 1.6 MB budget | [check_size.py](../tools/check_size.py) | The game links the generator only if that is measured and worth it |

## What to take from kkrieger

.kkrieger shipped operators and parameters instead of pixels, and rebuilt its
textures at load. The public source of the same tool family is
[werkkzeug3's `genbitmap`](https://github.com/farbrausch/fr_public/blob/master/werkkzeug3/genbitmap.hpp).
It has operators such as `Flat`, `Perlin`, `Cell`, `Gradient`, `GlowRect`,
`Dots`, `Bricks`, `Text`, `Merge`, `Mask`, `Paste`, `Color`, `Range`, `HSCB`,
`ColorBalance`, `Blur`, `Sharpen`, `Rotate`, `Twirl`, `Distort`, `Bulge`,
`Normals`, `Light` and `Bump`, on 64-bit pixels (16 bits per channel, inferred
from the types). The page UI and the store and load operators described below
are from memory of the tool, to confirm against its source in M12.6.

| Idea | Adopted as |
|---|---|
| One pixel format for every operator | RGBA at 16 bits per channel, so any output feeds any input. Grey data has R = G = B |
| Parameters, never pixels | Recipes are text files. PNGs are a build product |
| Operators stacked on a page, where the input is the block above | The graph editor, so there are no wires to route and cycles cannot be drawn |
| Named store and load | How one mask drives both albedo and height, as `mortar` does in `brick()` |
| Colour shaded from height | A `Bump` operator that bakes light and cavity into albedo |
| Intermediate results freed by reference count | The evaluator, plus a cache for interactive edits |

Different on purpose: hand drawing is added, stored as replayable strokes.
Coordinates are normalized, so recipes do not depend on resolution. Everything
tiles by construction.

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
- **Determinism.** The same recipe gives the same bytes on Windows and LazyOS.
  Kernels use `+ - * /` and `sqrt`; any `sin`, `cos` or `pow` comes from tables
  built once by an in-repo series. All randomness is a seeded `mscent_math` Rng
  per operator, so changing one seed never disturbs another operator.
- **Tiling.** Every generator is periodic and every filter samples with wrap.
  Parameters that would break the period, such as noise frequency, are integer
  tile counts.
- **Hostile input.** Recipes are untrusted, like levels. Caps: 256 operators,
  1024 px per side and powers of two only, 4096 strokes per paint operator,
  bounded octaves, cells and dots, and a memory limit checked before allocation.
  Evaluation is iterative, not recursive. Errors are returned, never panics.

### Recipe format

Text, diffable, and in the spirit of `level_text`. Layout is the graph: an
operator's input is whatever sits directly above it, in columns `x..x+w`.

```
miniscent-recipe 1
name brick
size 128 128
gloss 25
# x y w  op
op 0 0 1 bricks rows=8 cols=4 offset=0.5 mortar=0.0625 mode=mask seed=7
op 0 1 1 store  name=mortar
op 1 0 1 noise  freq=8 octaves=5 persistence=0.55 seed=1
op 1 1 1 store  name=grain
op 0 3 2 mask   # ...
op 0 9 1 out-albedo
op 1 9 1 out-height normal_strength=5
```

### Operators

| Group | First set (M12.2) | Later set (M12.9) |
|---|---|---|
| Noise and patterns | `flat`, `noise` (fbm, ridged, separate x and y frequency for brushed metal), `bricks` (mask, per-brick tint and bevel modes), `checker`, `stripes`, `gradient`, `glow` | `cells` (Voronoi), `wavelet` |
| Drawing | `rect`, `ellipse`, `line`, `dots` (grid or scattered stamps, for rivets), `paint` (M12.8) | `text` with the in-repo bitmap font, `scratches` |
| Filters | `color`, `range`, `invert`, `hscb`, `blur` | `sharpen`, `rotozoom`, `twirl`, `bulge`, `distort` |
| Combine | `merge` (add, subtract, multiply, min, max, screen, blend), `mask`, `paste` | |
| Height tools | | `bump` (bake light into albedo), `cavity`, `slope` (specular from height) |
| Flow | `store`, `load`, `out-albedo`, `out-height`, `out-specular` | |

A recipe needs `out-albedo`. Without `out-height` the normal map is flat, and
without `out-specular` the mask is full. Height and specular read channel R.

## Engine changes

1. **Stable ids.** A new `assets/textures/materials.txt` lists `id name gloss`,
   and it decides the ids. Stock ids 0 to 4 do not change. Ids are append-only:
   deleting a material leaves a retired id, so existing levels keep working.
   Names are checked (`[a-z0-9-]`, no `_n`, `_h` or `_s` suffix) since they
   become file paths. `default_gloss` goes away.
2. **Specular map (finishes M5.7).** On load, `_s.png` goes into the alpha of
   the normal texels, which are already fetched per pixel, so there is no second
   texture fetch. The highlight is multiplied by that alpha, only in the
   `SPEC = true` shader. PNGs without `_s` have alpha 255, so existing materials
   must render the same pixels; a test asserts it. The mask fades to the
   material's mean with `bump_scale`, because the normal array has no mips and
   an unfiltered mask would shimmer. M12.5 must also handle the paths that skip
   the normal fetch.
3. **Reloadable table.** `MaterialTable` holds `Arc<Material>` and the editor
   keeps a table plus a generation number in place of the `OnceLock`. A new
   generation updates the preview through the existing preview path and keeps
   the lightmap.
4. **Dynamic material list.** The brush panel and `texture_sizes` read the
   table, not `NAMES`.

## Editor workspace

A third child of the existing `stack`, opened from a new Texture menu. The level
views hide while it is open, and the level document is untouched.

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
- **Lit view.** A tile lit by an orbiting light through the engine's own
  `view_normal`, `bump_gain` and `SpecTable`, so it matches the game's response
  with no level or bake.
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
| M12.1 | `mscent-texgen`: `Rgba16`, wrapped sampling, tables, periodic noise; `texgen_bench` example | | M | Seam test (edge difference matches interior difference) for every frequency; same bytes twice; noise at 128 px timed |
| M12.2 | First operator set and the parameter table | M12.1 | L | Per-operator tests read real pixels; a seam test runs on every operator, including chains; hostile parameters do not panic |
| M12.3 | Pages to DAG, evaluation, refcounts, cache, recipe text format | M12.2 | L | Round trip; bit-flip and truncation fuzz; store and load misuse rejected; memory limit enforced before allocating |
| M12.4 | Output pack and headless bake: `mscent-editor --bake-texture <recipe> <dir>` | M12.3 | M | The normal of a test height matches `to_normal`'s values; PNGs load through `load_texture`; an agent can make a material with no window |
| M12.5 | Engine: `materials.txt`, specular in normal alpha, gloss from the manifest, `Arc` table, dynamic lists | | M | Adding a material leaves stock ids and existing-level pixels unchanged; forged manifests return errors; a masked highlight differs by pixel test; `check_frame_cost.py` and `crawl`/`stability` before and after |
| M12.6 | Workspace shell: list, op page, parameters, preview, undo, worker | M12.3 | L | `--selftest` builds a recipe through messages and reads preview pixels; worst UI tick under 1 ms during a drag (`tick_costs`) |
| M12.7 | Lit view, Save and Apply, reload into the 3D view | M12.4, M12.5, M12.6 | M | Selftest applies a change and the 3D view's pixels change while the lightmap key does not; the saved material loads in the game |
| M12.8 | Paint operator and tool | M12.6 | M | Replay at 128 and 512 px agrees after downsampling; a stroke across the edge has no seam; undo restores identical pixels |
| M12.9 | Later operator set, starter templates | M12.2 | L | Same tests as M12.2; each template renders in a selftest contact sheet |
| M12.10 | Stock recipes for all five materials, retire `gen_textures.py` | M12.7, M12.9 | M | Side-by-side contact sheet reviewed by you; full test suite passes, with snapshot changes explained; CI checks that committed PNGs equal the recipe output |
| M12.11 | Optional: game builds materials from recipes at load | M12.10 | M | Decided on measured numbers: binary size against the budget, load time against the 67 ms PNG decode noted in `build.rs` |

M12.5 is independent of the rest and can start at any time, so it can run in
parallel with M12.1 to M12.4. A first usable release is M12.1 to M12.7.

Targets to check against `texgen_bench`: all five stock recipes at 128 px in
under 100 ms total on the dev machine, and one parameter change at 256 px under
50 ms. Numbers on the i3-7100U go in `docs/perf/`.

## Risks

- **Stock materials shift.** Rust noise cannot reproduce numpy's FFT noise bit
  for bit, so M12.10 gives lookalikes, and snapshot tests against stock textures
  will change. PNGs stay as they are until you accept the new ones.
- **Specular cost.** The mask adds little in theory, since no fetch is added,
  but High mode has a tight budget. If `check_frame_cost.py` objects, the mask
  becomes per-material again.
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
