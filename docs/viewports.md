# Viewport surfaces: plan

Status: draft, nothing here is implemented. Time and size figures are targets
to measure, not results. The work items below move to
[work-items.md](work-items.md) as M13 once this is accepted.

## Goal

Let a level look out into something far larger than itself: the planet behind
UT's Facing Worlds, the layered skies of the original Unreal, a canyon past the
courtyard wall. A mapper paints a brush face with a **viewport material**, builds
the view in a sealed room with the ordinary brush tools, and drops one entity in
that room. Nothing else is needed.

More general than a Half-Life skybox, because what is seen is real level geometry
with its own lighting, motion and parallax. Much less than a portal system,
because the view never recurses and nothing passes through.

Not in scope:

- Recursion, mirrors, or a view from a viewport into another viewport.
- Travel, shots or sight through a viewport. It is a wall to the game.
- Render to texture, monitors and security cameras.
- Actors, movers or particles inside a view (see "Later"). Voxels are drawn.
- Light coming in from the sky (see "Later").
- A hardware path, or prebaked cubemaps.

## What exists today

| Fact | Where | What it means here |
|---|---|---|
| There is no sky. Levels are sealed rooms, and the gradient only shows where something failed | `clear_sky` in [framebuffer.rs](../crates/mscent-render/src/framebuffer.rs); pinhole tests set `sky_top: HOLE` | The feature starts from nothing, and the pinhole tests must keep their meaning |
| The world starts solid, and rooms are subtractions | [plan.md](plan.md#csg-and-bsp) | A sealed room far from the play space is already possible, and no portal joins it to the rest |
| The flood starts at the eye's leaf and takes a screen rectangle | `visible_leaves` in [visible.rs](../crates/mscent-render/src/scene/visible.rs) | It can start from the view's leaf with a narrower window. An eye in solid marks **every** leaf visible, which a view must not do |
| A frame lists once and fills per band: clear, movers, voxels, faces, actors, particles | `render_with_effects` in [level/mod.rs](../crates/mscent-render/src/level/mod.rs) | The new work slots in without touching existing passes. That file is at 488 lines, so it goes in a new module |
| Depth is `1/z`, 0.0 means nothing drawn, and the test is `iz > depth` | `span_pixels` in [raster.rs](../crates/mscent-render/src/raster.rs) | Setting depth to infinity blocks a pixel with no rasterizer change |
| Colour alpha is kept at 255 by every shader | `mul_rgb`, `lerp_rgb` in [shade.rs](../crates/mscent-render/src/shade.rs) | Alpha 0 is free to mark a pixel for the length of a pass |
| `FaceShader` is a `match` per pixel | [level/shader.rs](../crates/mscent-render/src/level/shader.rs) | A new variant would touch the hot path, so viewport faces get their own list |
| `NO_DRAW = 0xFFFF` is a material that is not a texture | [materials.rs](../crates/mscent-world/src/materials.rs) | Reserved ids work, and need no new face data |
| A shot with no face under it has no impact and no scorch | `surface_at`, `face_at` in [shots.rs](../apps/game/src/session/shots.rs) | Keeping viewport faces out of `Scene::faces` gives the right behaviour for free |
| `FAR` is 400 and `MAX_FOG_DEPTH` is 1000 | level/mod.rs, [entity.rs](../crates/mscent-world/src/entity.rs) | A view needs its own far plane and fog limit |
| Luxels default to 0.125 units, the atlas is at most 1024x8192 (8.4 M), and an overfull atlas coarsens the **whole level** | `BakeConfig` in [lightmap/mod.rs](../crates/mscent-world/src/lightmap/mod.rs) | One 400-unit box wants 61 M luxels. A big view would blur the play space. This is the largest trap here |
| `lamp_light` and mesh lighting use every lamp, without shadows | [compiled.rs](../crates/mscent-render/src/level/compiled.rs), `mesh_lighting` | A huge lamp in a view would light enemies in the play space |
| Mips and bilinear sampling force alpha to 255, and the generator writes RGB at 128x128 | [mips.rs](../crates/mscent-render/src/mips.rs), `texture.rs:368`, [gen_textures.py](../tools/gen_textures.py) | Cutout textures need an alpha path and a generator change |
| `lightpanel` is the only emissive material, chosen by id | `face_shader` in [fill.rs](../crates/mscent-render/src/level/fill.rs) | Self-lit sky materials follow that precedent, without a lightmap |
| Materials are per face in the editor and in level text | `TexInfo`, [level_text.rs](../crates/mscent-world/src/level_text.rs) | Painting a viewport needs only new entries in the lists |

## The model

Three ideas, none of which adds a new kind of geometry:

- **Viewport face.** A face whose material is `VIEW_FIRST + slot`
  (`0xFFF0..=0xFFF7`, so 8 slots). It collides like any wall. On screen it is a
  hole showing that slot's vista.
- **Vista.** What a slot shows: up to 4 layers, drawn far to near.
- **Layer.** One new entity, placed in a sealed room:

```
Entity::VistaLayer {
    pos,         // the layer's eye when the player stands at `centre`
    centre,      // the point of the play space that maps to `pos`
    slot, layer, // which vista, and the order inside it (0 is farthest)
    parallax,    // 0..=1: how far the eye moves per unit the player moves
    spin,        // axis times radians per second, turns the layer's camera
    pan,         // texels per second, on self-lit faces only
    fog_start, fog_end, fog_color,
}
```

The eye of a layer follows one line:

```
eye = pos + (player - centre) * parallax
orientation = spin_rotation(time) * player_orientation
```

`time` is `Effects::vista_time`, in seconds. The game passes its fixed step count
over `STEP_HZ`, plus the part of the next step, so it stands still when play does
and a scripted shot is the same every run; the editor's 3D view leaves it at 0, and
a negative or non-finite value counts as 0. `spin_rotation` turns about the axis of
`spin` by `|spin| * time`, with whole turns dropped; zero spin leaves the player's
orientation untouched. A positive `pan` slides the picture of a self-lit face towards
+U and +V of its texture axes, by `pan * time` texels, whole textures dropped.

`parallax` is the only scale control, and it covers each style. People think of it
as **scale**, its inverse (`scale = 1 / parallax`): level text takes `x16` where the
parallax goes, and the editor panel has a Scale field (see "Scale" below).

| `parallax` | Reads as | Reference |
|---|---|---|
| 0 | Infinitely far: turns with the head, never shifts | Unreal 1 sky zone, Half-Life sky |
| 1/16 | A miniature that looks 16 times its size, and shifts as you fly | Source's 3D skybox |
| 1 | A window on a same-size place (no sense of scale, rarely wanted) | |

A room 100 units wide at 1/16 reads as 1600 units across. That is how
monumentality fits the lightmap, the coordinates and the bake time. The same line
makes a miniature continue the real floor exactly: a plane at height `z` in the
play space lines up with `pos.z + (z - centre.z) * parallax` in the view.

A level with no viewport face does no new work and draws the same pixels.

### Scale

A room at scale `s` is authored `s` times smaller than it looks. The stored field
stays `parallax`; scale is how it is set:

- **Level text:** `vista` takes `x16` in the parallax position, meaning 1/16. It is
  `x` and a finite number of at least 1, so `x0` is an error. Dumping writes the
  parallax, which reads back to the same bits.
- **Editor panel:** the field is "Scale (0 = infinite)": 1 to 4096, clamped, and 0
  for parallax 0 (infinitely far). Undo covers it like any field.
- **Luxels follow scale.** A face in a layer's room bakes `luxel_size * f` where
  `f = VIEW_LUXEL_SCALE / scale`, never under 1, so a luxel looks the same size
  whatever the scale; an infinite layer keeps `VIEW_LUXEL_SCALE`. Layers sharing a
  room use the largest `f`. `VIEW_LUXEL_SCALE` is 8 (one-unit luxels at scale 1):
  at 32 a two-unit pole's shadow washed out to 5% of its depth, at 8 it keeps 80%.
  A face's luxels per side are capped at `MAX_FACE_LUXELS` (128), so a big room
  cannot take more atlas than that however fine `f` is, and a floor wider than 128
  luxels coarsens to fit. `vista_scale` in `mscent-world` holds the rule.

| Scale | `f` | Luxel at `luxel_size` 0.125 |
|---|---|---|
| infinite, 1 | 8 | 1 unit |
| 2 | 4 | 0.5 |
| 4 | 2 | 0.25 |
| 8 and more | 1 | 0.125 |

What scale saves is the room's size, so cells and faces: the same apparent range of
mountains, 256 across, built at scale 1 (one-unit cells in a 288-unit room) and at
scale 16 (cells of 1/8 in an 18-unit room) measured by the `scale_lightmap` example:

| Scale | `f` | Faces | Atlas luxels | Voxel cell records | Chunks | Bake |
|---|---|---|---|---|---|---|
| 1 | 8 | 22 | 70 688 | 116 085 | 1280 | 844 ms |
| 16 | 1 | 22 | 63 120 | 29 579 | 192 | 224 ms |

The atlas is the same because both rooms' big faces reach the 128-luxel cap, one at
2.25 units a luxel and the other at 0.14, which both look 2.25 units wide. The
saving is in the voxel cells (3.9 times fewer) and the bake (3.8 times quicker). At
scale 1 a range wider than 256 units cannot exist at all: a volume is at most 16
chunks of 16 cells a side, and a level at most 65 536 chunks.

## Why this and not the alternatives

| Option | Verdict |
|---|---|
| Hardcoded sky cube (Half-Life) | No geometry, no parallax, no layers. It is what a one-room vista with `parallax` 0 already gives |
| Prerendered cubemap | Static, no parallax, at least six large textures, a new sampler per pixel |
| Render the view to a texture | Pays for a whole screen, then resamples it. Worse in every way on a software renderer |
| Real portals with clip planes | Needs per-span clipping and recursion rules, for effects this feature does not want |
| Vista first, main scene over it | Simple, but shades every vista pixel the level then hides, which breaks the frame budget |

## Frame order

Main scene first, so the view shades only the pixels that show. The rasterizer does
not change.

```
list main faces, with viewport faces going to their own list
if that list is empty: frame exactly as today
for each slot seen, each layer: list its faces with its camera, flood window = hole bounds

per band:
  clear_sky, movers, voxels, faces, actors       unchanged
  fill viewport faces: true depth, colour 0      closer pixels already won the z-test
  for each slot with a hole in the band:
    resolve: this slot's mark -> gradient colour, view depth 0
             anything else    -> view depth infinity
    swap depth planes
    for each layer: fill, then reset view depth that is not infinity
    swap depth planes back
  particles                                      test against the main depth
```

- **The hole writes true depth.** The flood is conservative, so faces beyond a
  viewport wall can be listed. They must stay hidden.
- **Infinity is the mask.** Every covered pixel fails `iz > depth`, so it costs a
  compare and shades nothing. The view's own overdraw is bounded by its z-test.
- **The second depth plane** is made once per band, and only in a level with
  viewports. Particles and the composited depth still see the main scene.
- **Uncovered hole pixels show the gradient,** so an unbound slot fails visibly, and
  a closed backdrop can be tested for pinholes the way voxels are.
- **Alpha returns to 255 before the pass ends.** Nothing outside this module
  ever sees the mark.
- **Several slots in one frame** need telling apart, so the mark is alpha 0 with
  the slot in the low colour bits, and each slot resolves in turn.

A layer is shaded at the frame's quality, like the play space: lightmap, bump and
highlights, with the highlights taken from the layer's own eye. Self-lit faces stay
flat. Dynamic lights stay off, since they cannot reach a sealed room. Its voxels
are filled before its faces, into the same masked depth: a covered pixel has depth
infinity, which both voxel fills reject.

### A viewport face inside a layer

The walk skips it, so it is **see-through to the layers behind**. This rules out
recursion by construction, and it is what makes layers easy: paint an arena room's
walls and ceiling with any `view:` material and the star layer shows above its
towers. No special "clear" texture is needed.

## Islands

An island is a set of empty leaves joined by portals, computed once per build with
a flood over `Portals`. Each layer belongs to the island around `pos`.

- The layer's flood stays in its island. If the eye has moved out of it (small
  rooms, high parallax) every leaf **of the island** counts as visible, and frustum
  and back-face culling do the rest. The whole-level fallback is not used.
- Lamps in a layer's island that is not the player start's leave `mesh_lighting`.
  A voxel surface in such a room, made by a live edit before the next bake, is lit
  from every lamp, since that is what the bake lit it with; one in the play space
  from the play lamps only. The bake keeps them all, since solid stops the rays and a
  key made of the same lamps stays valid. Skip them per luxel only if bake time
  shows it matters.
- A face in such an island bakes at a coarser luxel (see "Scale"), as
  `LuxelDensity::Coarse` on its `LightFace`. A
  self-lit face is `LuxelDensity::None`: an empty rectangle in the atlas, no rays,
  no bounce. The input key hashes a density only when it is not `Full`, so
  existing caches stay valid. The play space keeps its density whatever the
  view's size. A level with no player start treats no island as the player's.

Editor and `--check` warnings, not errors: the view shares an island with the player
start (a leak), a layer in solid, a viewport face on a slot with no layer, a mover
with a viewport face. `vista_warnings` in
`mscent-world` finds them; the editor puts them in its log when they change. A sky or
cutout material on a voxel volume is not checked.

## Self-lit and cutout materials

**Self-lit.** `materials::is_self_lit(id)`, a table of the game's own materials like
`is_destructible`. These faces draw through `FlatShader` and get **no luxels**, so a
starfield room of any size costs no atlas and no rays. It is also the cheapest pixel
the renderer has. `lightpanel` stays as it is, to keep existing pixels.

**Cutout.** A texture whose PNG has any alpha below 255 is masked, found at load.
Its alpha lives in a separate plane with its own mips, so opaque textures keep today's
memory and sampler. `draw_row` picks a second span loop per span:

| Texel alpha | Result | Depth |
|---|---|---|
| 0 | Skipped | Unwritten |
| 255 | Written | Written |
| Between | Blended over the colour there | Unwritten |

That gives clean silhouettes, soft cloud edges and no depth sorting. The cost of one
branch per span is small, and `check_frame_cost.py` confirms it. Within one layer,
drawing is front to back, so a soft edge over something drawn later loses to it.
Anything that must blend over a thing goes in a later layer.

Cutouts are only guaranteed in layers. In the play space they would need the same
care around lightmaps and actors, and nothing asks for that.

## Sky textures

Four new stock materials, each one job, designed to stack. They are self-lit, tile
as noted, and come from `gen_textures.py` with the existing noise helpers. The
generator gets an RGBA `save` and a size argument, with colour bled under transparent
texels so mips do not fringe. `_h` and `_n` stay flat so the loader's rules hold.

| Id | Name | Size | Alpha | Tiles | Use on a brush |
|---|---|---|---|---|---|
| 10 | `sky-stars` | 512 | Opaque | Both | The inside of the room: the backdrop. Faint nebula noise, stars at least 2 texels so bilinear does not make them flicker |
| 11 | `sky-clouds` | 512 | Soft | Both | A ceiling plane or big plate in a later layer, panned. Fractal noise thresholded to alpha |
| 12 | `sky-ridge` | 512x256 | Hard top edge | U | A ring of thin brushes round the room: a mountain line, opaque below a fractal ridge. Fit V once with `TexInfo` |
| 13 | `sky-planet` | 256 | Hard disc | No | One face of a plate, fit once: Facing Worlds' Earth. Limb darkening, cloud bands |

Each gets one line in `materials.txt`, at the end, so no id moves. Names use a hyphen:
the manifest allows no `_` in a name, which keeps a name from reading as a companion
file (`_n`, `_h`, `_s`). The constants in `materials.rs` stay `SKY_STARS` and so on.
The loader takes 512x256 as it is. Each texture comes with a flat `_h` and `_n`, and
the generator draws them from their own random stream, so the older textures are
byte for byte as they were.

Cutouts draw only through the rasterizer's polygon path. A voxel face with a cutout
texture draws its colour and ignores the alpha.

## Building one with the existing tools

No new brush tool. New: one entity, four textures, and the viewport entries in the
material lists.

**Facing Worlds: a planet behind a floating platform.**

1. Subtract a big box far from the level, and paint it `sky-stars`.
2. Add a thin plate near one wall, and paint its face `sky-planet` with Fit.
3. Place a Vista Layer in the box: slot 0, layer 0, `parallax` 0, a slow `spin`.
4. In the play space, select the room's walls and ceiling, and pick **View 0** in
   the face panel. Add the platform brushes in rock as usual.

**Monument: a huge arena past a low wall.** Do 1 to 3 above for the sky, then:

1. Subtract a second room, 100 units wide. Paint the walls and ceiling **View 0**.
   They become see-through to the stars.
2. Add a floor, towers and arches with the ordinary tools, then a ring of plates in
   `sky-ridge`, and lamps.
3. Place a Vista Layer in it: slot 0, layer 1, `parallax` 1/16, with fog at the far
   end for depth.

**Mountains: a range past the platform.** **New mountains** does this in one step. By
hand: subtract a 128-unit room with View walls, add a floor, drop in a volume from
`mountains` (or the Mountains generator), a lamp, and a layer at scale 16 (`x16`) of
the same slot, after the stars. The demo's plain has three such ranges, one on each
flank and one behind the gate, which leave the avenue of towers clear.

**Original Unreal: clouds over a horizon.** Add a third room with one big plate in
`sky-clouds`, layer 2, a `pan`, walls painted **View 0**. It costs no lightmap.

An editor command, **New Vista** (Edit menu), does the first recipe in one step: a
64-unit room of stars 32 units beyond the level bounds, a planet plate 8 units off
its west wall, and a layer for the next free slot with a slow spin. `centre` is the
bounds' centre, or the `centre` of a layer the level already has, so all layers agree.
It goes through the normal commands, so undo covers it, and with all 8 slots used it
does nothing and says so. Then the mapper only paints faces: **View 0** to **View 7**
are the last rows of both material lists. The panel edits the fog colour as three
channels, and every field is held to the ranges of `Entity::is_valid`.

## Voxels and mountains in a layer

A layer has a `VoxelList` of its own. The walk that lists its faces also collects the
voxel chunks of the island's visible leaves, so another room's voxels never show, and
the list is prepared from the layer's camera and eye. The distance a chunk switches
level of detail at is measured from that eye in the room's units, so a miniature picks
coarse levels by itself: at scale 16 a range 100 units off is 1600 away. Budgets: a
layer's list may submit `LAYER_VOXEL_TRIS` (12 000) before the rest take the lowest
level, all layers together `VISTA_VOXEL_TRIS` (24 000), and the main scene keeps its
own `MAX_VOXEL_TRIS`. The lowest level still has a cost (cells on a chunk's border never
merge), so a frame can pass these; the demo submits 70 253 triangles to its layers at
the gate and 33 400 looking up. `SceneStats::vista_triangles` reports it.

`mountains` in `mscent-world` (`voxel/mountains.rs`) fills an add volume from a ridged
heightfield: crests where noise crosses zero, a broad mask for foothills, an envelope
that takes the surface to the ground at the box's edges, solid below. It is pure, built
from integer-hash noise and IEEE arithmetic only, so the same `MountainRange` always
gives the same bytes. The box grows to whole chunks; at most 16 chunks a side, so 256
units at one-unit cells, which is the cell to choose for a distant range. Bad input is
a `MountainError`. Level text has a line for it, expanded at parse into a `Volume`:

```
mountains <order> <material> <min x y z> <max x y z> <cell> <peak> <seed>
```

Dumping cannot turn a volume back into this line (a sculpted range is no longer one),
so `--dump-level` refuses a level with volumes, as it did.

The editor has a Mountains generator beside the others (it builds a whole volume at the
aimed place, wide in proportion to the radius), and **New mountains** (Edit menu), which
adds a miniature room at scale 16 beyond the level: a sealed 128-unit room whose walls
and ceiling are the slot's View material, a rock floor, a range, one large lamp, and a
layer that is the next of the existing vista's slot (or the next free slot). The layer's
height makes the room's floor continue the play floor. One undo step; with the slot's
layers full it does nothing and says so.

## Costs and budget

The reference machine still holds 60 fps with every feature on. Targets to measure at
640x360, one worker:

| Step | Target |
|---|---|
| No viewport face in view | Adds one empty-list check. `frame_bench` unchanged |
| Filling viewport faces plus resolve | Under 0.3 ms at full-screen hole (`simd.rs` if needed) |
| Flood and listing, per layer | Under 0.3 ms for a room of 64 brushes |
| Layer pixels | No dearer than a flat-shaded wall for self-lit, than a Low wall for lit |
| Lightmap | A 100-unit lit room under 5 % of the atlas (at 0.125 it would take 46 %) |

Hole pixels replace main-scene pixels, so a mostly sky view has fewer brush pixels to
pay for. `SceneStats` gains `vista_pixels` and `vista_faces`, shown beside the existing
figures. A voxel-style governor is not planned; 4 layers and the 64-brush guidance
are the budget until a level shows otherwise.

`tests/frame_alloc.rs` keeps its count on levels without viewports. With them, polygon
lists and the second depth plane are made once; a test holds the per-frame count to
the flood's existing vectors per layer.

## Risks

- **Far geometry at a distance.** Thin brushes alias at 640x360 and shimmer as the
  player moves. Check `crawl` and `stability` on the demo level, and prefer plates with
  cutouts over thin real geometry.
- **Depth range.** Layer far is 100 000. `1/z` in `f32` holds this well because the
  precision grows toward zero, and depth is cleared per layer, but M13.2 checks for
  z-fighting at range.
- **Box corners on a sky.** A flat-lit `sky-stars` box can show its corners. Stars hide
  them, and a dome built with `hull_planes` (64 planes at most) is available if not.
- **The main fog does not touch the hole.** A wall fogged in the play space can stand
  against a clear sky. Authors match the layer's fog colour. Revisit only if it reads
  badly in the demo.
- **Spin and replay.** Time reaches the renderer through `Effects`, read-only, and only
  moves cameras. `Game::step` never sees it, so replays stay bit-exact. Screenshots
  with time 0 are stable.

## Tests

Pixel reads through the renderer, as everywhere else:

- Levels with no viewport face: byte-identical pictures, depth and lightmap keys.
- Same pixels and depth at 1 and N workers, and across band sizes.
- `parallax` 0: a landmark holds its screen position while the eye translates, and moves
  when it turns. `parallax` 1/16: it moves exactly 1/16 as far as at `parallax` 1.
- An actor, door or voxel in front of a hole keeps its pixels, and a face listed beyond
  a viewport wall shows none. A particle blends over a hole and is hidden behind a wall.
- Layers: a cutout texel shows the layer behind, a `view:` face in a layer shows the
  layer behind, a soft alpha reads between the two colours.
- Eye outside its island: no pixel from any other island, over 1000 random poses.
- Closed backdrop, 1000 poses at several sizes: no gradient pixel.
- A huge lamp in a view: no change to a play-space lightmap or to an actor's light.
- Adding a large view leaves the play space's luxel size unchanged.
- A shot at a viewport face: no scorch, no impact, the same collision.
- Format: round trip, versions 1 to 4 still load, tag 7 in an old version rejected, bit
  flips and out-of-range fields never panic. A slot above 7 or a NaN `parallax` is an
  `Err`. A duplicate slot and layer: the first counts, like `Entity::World`.

## Work items

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M13.1 | World: `VIEW_FIRST`, `Entity::VistaLayer` as tag 7, format version 5, level text, islands | M2.6 | M | Round trip; hostile files return errors; v1 to v4 load; islands agree with a flood on generated levels |
| M13.2 | Renderer: viewport face list, resolve, second depth plane, single-layer vista of lit faces, camera line, island flood | M13.1 | L | The tests above on occlusion, parallax, workers and islands; no-viewport levels identical; `check_frame_cost.py` before and after |
| M13.3 | Bake: coarse luxels by island, lamps split by island, self-lit faces without luxels | M13.1 | M | The lamp and luxel-size tests; old caches still load; the atlas figure recorded |
| M13.4 | Masked textures: alpha plane and mips, second span loop, RGBA generator, the four materials | M13.2 | M | Alpha tests; opaque textures use unchanged code and pixels; a texture sheet committed; frame cost compared |
| M13.5 | Layers, `spin`, `pan`, per-layer fog, `Effects` time, stats | M13.2, M13.4 | M | Layer tests; a replay trace unchanged; screenshots stable at time 0 |
| M13.6 | Editor: entity tool and panel, View entries in the material lists, warnings, New Vista, 3D view shows the sky | M13.3, M13.5 | M | `--selftest` builds a vista, paints a wall, plays it, reads a sky pixel, and undoes to the same picture |
| M13.7 | `assets/levels/facing.txt` demo with the Monument recipe, flythrough script, measurements in `docs/perf/` | M13.6 | S | Holds budget on the dev machine; `crawl` and `stability` checked; screenshots in the PR |

M13.2, M13.3 and M13.4 can run side by side. M13.2 alone already gives a working
single-layer view, usable from level text.

## Later

Each stands on its own and none is needed for the goal:

- **Sky light.** Viewport faces lighting a room during the bake. Until then, a far lamp
  with a large radius plays the sun.
- **Actors and movers in a view:** a passing ship, a turning wheel, driven by script.
- **Script control:** switch a slot's layers, or change `spin`, for a day that turns to
  night.
- **Custom self-lit materials,** from a manifest flag, with the Texture Studio.
