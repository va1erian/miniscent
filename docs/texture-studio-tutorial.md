# Texture Studio tutorial

This walks you through making a material from nothing, slate floor tiles with a
crack drawn by hand, and using it in a level. It takes about fifteen minutes. A
test bakes the finished recipe at the end of this page, so it stays correct.

Start the editor and open the studio:

```bash
cargo run --release -p mscent-editor
```

Choose **Texture > Texture Studio**. **Back to the level** returns you to your
level, and nothing you do here changes the level until you apply a material.

## How the page works

The lower middle of the window is the **page**. Every operator is a block on a
grid, and there are no cables. The rule is position:

> A block reads the blocks on the row directly above it that overlap its columns,
> left to right.

So a block one column wide reads one block, and a block dragged out to two columns
can read two. Each input a block really reads shows as a small black mark on its
top edge, and the right-hand panel says it in words, such as "reads 2 inputs:
noise, bricks".

Mouse controls on the page:

| Do this | To |
|---|---|
| Right click an empty cell | Add a block there, from a menu grouped by kind |
| Click a block | Select it, show its parameters on the right |
| Drag a block | Move it |
| Drag a block's right edge | Make it wider or narrower |
| Right click a block | Duplicate or delete it |
| Wheel, or middle-button drag | Scroll a long page |
| **Esc** while dragging a block | Cancel the drag. A second **Esc** goes back to the level |

**Ctrl+Z** and **Ctrl+Y** undo and redo in the recipe, not in your level.

## 1. Make a new material

Click **New...**, name it `slate`, and keep the size at 128. You get a starter page:
a `flat` block with an `out-albedo` under it. `out-albedo` means "this image is the
material's colour".

Size is worth choosing now. A texture's size in the world is its texel count times
the face's scale, so changing it later rescales the texture on every face using it.

## 2. Noise for the stone

1. Select the `flat` block and press **Del**.
2. Right click the empty cell at the top left and choose **Pattern > noise**.
3. In the panel, set `freq_x` and `freq_y` to 6, `octaves` to 4, and `seed` to 5.

The preview shows grey clouds. Each block has its own small picture, and the
**Block** tab shows the selected one, so you can inspect any stage of a recipe.

Noise blends from `color0` to `color1`, so colour it directly:

- `color0`: 0.2, 0.24, 0.28
- `color1`: 0.42, 0.46, 0.5

## 3. Tiles

Right click column 1, row 0, and add **Pattern > bricks**. Set `rows` and `cols` to
4, `offset` to 0, which gives a straight grid, and `mortar` to 0.08. This is a mask:
white tiles, black gaps.

Other blocks will want this mask, and they can't all sit under it. Add **Flow >
store** under it at column 1, row 1, and set its `name` to `tiles`. A `load` block
anywhere on the page with the same name reads it. This is how one pattern drives
colour, height and shine, and it's the only way to reach further than one row.

## 4. Combine them

The merge has to sit under both the stone and the `store`, so make room:

1. Drag `out-albedo` down to column 0, row 3. The page shows an error for a moment,
   since it now has nothing above it, and that's fine.
2. Drag `noise` down to column 0, row 1, so it sits beside the `store`.
3. Add **Combine > merge** at column 0, row 2, and drag its right edge until it covers
   columns 0 and 1. Two marks appear on its top edge, because it now reads two blocks.
4. Set `mode` to `multiply`.

Click the **Albedo** tab: dark stone with black gaps between the tiles. Turn on **2x tiled** to check the edges. Every
operator tiles by construction, so you will not find a seam.

## 5. Height, then relief

The **Normal** map is worked out from height, so you never draw it.

1. In column 3, add **Flow > load** at row 0, with `name` set to `tiles`.
2. Under it at row 1, add **Filter > blur**, with `radius` 0.004, to round the edges.
3. Under that at row 2, add **Flow > out-height**, with `normal_strength` 4.

Look at the **Height** and **Normal** tabs. On **Lit**, a light orbits over the tile
using the game's own shading, so the relief shows as it will in play.

## 6. Shine

Specular says how much of the highlight each texel gets. The overall strength is the
**Gloss** slider on the left, which you can set to 60.

In column 4, add **Flow > load** at row 0 with `name` set to `tiles`, then **Flow >
out-specular** under it at row 1. Now the gaps have no shine, and the tiles do. On
**Lit**, the highlight sweeps over the tiles and stops at the gaps.

## 7. Draw a crack

1. Add **Draw > paint** at column 2, row 1, and set its `background` to white. On its
   own it is a blank white page, so multiplying by it changes nothing yet.
2. Widen the `merge` over columns 0 to 2. It shows three marks now.
3. Select `paint` and click the **Block** tab.
4. In the **Brush** section, set `radius` to 0.012, `hardness` to 0.8 and `colour` to
   0.15, 0.15, 0.15.
5. Drag on the preview to draw a crack. Cross the edge of the tile if you like, since
   the stroke continues on the other side.

A stroke is one undo step. A stroke is saved as coordinates, not pixels, so the recipe
stays small and the drawing stays sharp at any size. Its points are rounded to 1/4096
of the tile, and one stroke stops growing after about 2000 of them, with a message:
lift the pen and start another. If you draw so much that a `paint` block goes over its
work limit, the page says so, and **Ctrl+Z** recovers it.

## 8. Use it

Click **Apply**. The material joins the running editor. Click **Back to the level**,
click a face in the 3D view, and pick `slate` in the **Face / texture** section. Its
lighting doesn't re-bake, because textures don't affect the lightmap.

Then go back and click **Save**, which writes `assets/textures/slate.mst` and adds a
line to `assets/textures/materials.txt` that gives it a permanent id. Levels store
that id, so commit both files with any level that uses the material.

Materials are saved in the order they were applied, so an id means the same thing in
the running editor and on disk. If you applied another material first, Save refuses
and names it: save that one before this one. Save also refuses a new name when a
file `<name>.mst` is already in `assets/textures/` and `materials.txt` does not list
it, so it never writes over a file it did not make, and it asks first before saving
over any of the ten stock materials.

If you save a level that uses a material you applied but have not saved, the level
saves all the same and the status bar warns you, naming the material. Without the
material's line in `materials.txt`, those faces would draw grey next time, or show
another material once something else takes that id.

## Going further

- **A shape below a block draws over it.** `rect`, `ellipse`, `line`, `dots`, `text`,
  `paint` and `scratches` take an optional input. If one sits under another block, it
  draws over that block's image, not over its own `background`. If a shape looks wrong,
  check for a mark on its top edge. To start clean, leave the cell above it empty.
- **Generators ignore what's above them.** `noise`, `bricks` and the other patterns
  start a new chain, so they can sit under anything.
- **Sums need scaling.** Pixels stop at 1, so to add two things, turn them down first
  with `range` and `out_high`. The `color` block's `amount` can go above 1 to bring
  them back up.
- **Stop noise at the texel.** At 128 px, a frequency that doubles past about 64 adds
  only shimmer in the game. With `freq_x` at 8, stop at 4 octaves.
- **Learn from the stock materials.** Each one opens in the list as an example. Metal
  shows rivets made from `dots` and `rotozoom`, and floor shows a hazard band.

You can also write recipes as text and look at the result with no window:

```bash
cargo run --release -p mscent-editor -- --bake-texture assets/textures/slate.mst target/shots/slate
```

## The finished recipe

This is about what your `slate.mst` should hold, give or take your own crack. A block
is `op x y width name`, and anything not given takes its default.

```text
miniscent-recipe 1
size 128 128
gloss 60
op 0 1 1 noise freq_x=6 freq_y=6 octaves=4 seed=5 color0=0.2,0.24,0.28 color1=0.42,0.46,0.5
op 1 0 1 bricks rows=4 cols=4 offset=0 mortar=0.08
op 1 1 1 store name=tiles
op 2 1 1 paint background=1,1,1
stroke 0.012 0.8 0.15,0.15,0.15,1 0.1,0.2 0.35,0.45 0.6,0.4 1.1,0.75
op 0 2 3 merge mode=multiply
op 0 3 1 out-albedo
op 3 0 1 load name=tiles
op 3 1 1 blur radius=0.004
op 3 2 1 out-height normal_strength=4
op 4 0 1 load name=tiles
op 4 1 1 out-specular
```

## Operators

| Menu | Operator | What it does |
|---|---|---|
| Pattern | `flat` | One colour |
| Pattern | `noise` | Tiling clouds; `ridged` mode makes creases; a frequency of 0 on one axis makes streaks |
| Pattern | `bricks` | Running bond or a grid: a mask, a tone per brick, or a bevel |
| Pattern | `checker` | Alternating cells; counts must be even |
| Pattern | `stripes` | Bands, diagonal if both cycle counts are set |
| Pattern | `gradient` | A ramp up and back, so its ends meet |
| Pattern | `glow` | A soft spot |
| Pattern | `cells` | Voronoi: distance to the nearest point, cell edges, or a tone per cell |
| Pattern | `wavelet` | A second, more angular kind of noise |
| Draw | `rect`, `ellipse`, `line` | Shapes, with soft edges if you want them |
| Draw | `dots` | A grid of round stamps, optionally jittered: rivets, pits |
| Draw | `paint` | Hand-drawn strokes |
| Draw | `text` | Capital letters and digits in a small bitmap font |
| Draw | `scratches` | Random thin lines |
| Combine | `merge` | Two to eight inputs: add, subtract, multiply, min, max, screen, blend |
| Combine | `mask` | Three inputs: a, b, and a mask choosing between them |
| Combine | `paste` | Scales a second input into a rectangle of the first |
| Filter | `blur`, `sharpen` | Softens or crisps |
| Filter | `color` | Multiplies by a colour and an amount |
| Filter | `range` | Levels: input range, output range, gamma |
| Filter | `invert` | Flips colour |
| Filter | `hscb` | Hue, saturation, contrast, brightness |
| Filter | `rotozoom` | Quarter turns, whole-number repeats, and shifts |
| Filter | `twirl`, `bulge` | Warps from the centre that stop before the edge |
| Filter | `distort` | Two inputs: moves the first by the colours of the second |
| Height | `bump` | Two inputs: bakes light from a height into a colour |
| Height | `cavity` | Darkens crevices |
| Height | `slope` | Bright where a height is steep |
| Flow | `store`, `load` | Name an image, and read it anywhere on the page |
| Flow | `out-albedo` | This image is the colour |
| Flow | `out-height` | This image is the height; the normal map comes from it |
| Flow | `out-specular` | This image says where the highlight goes |

A recipe needs `out-albedo`. Without `out-height` the material is flat, and without
`out-specular` the highlight falls evenly.
