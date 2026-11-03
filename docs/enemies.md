# Enemy foundry: design

Status: accepted, and M15.1 and M15.2 are built ([as built](#as-built)). The renderer can draw a model, but
the game does not ask it to until M15.4, so it looks as it did. The numbers under [What the spike showed](#what-the-spike-showed) are measured on the dev machine
by a throwaway spike that is not in the tree. Figures for anything not yet built are targets. Work items are
[M15](work-items.md#m15-enemy-foundry).

## As built

M15.1, `crates/mscent-world/src/model/`, with three hand-written blueprints in `assets/enemies/`.

- **Format.** As below, with these changes from the design: a part's options hold no spaces, so a point is
  `pivot=8,4.5,1`; a role always comes last on its line, followed by `detail` on a solid; a `cut` may name a
  role for the walls it leaves, or takes the first skin's. A piece names a role, so skins come before parts.
- **Limits.** 64 KiB, 16 parts, 512 pieces, 64 brushes in a part with cuts, 8 hit spheres, 16 roles, 8 corners on
  a plate and 256 model units. Chunks are limited to 256, not 64: a box counts its empty chunks, at about 20 us each,
  and `MAX_MODEL_NS` is what bounds the work.
- **Levels of detail fall back.** Merging cells folds a limb a few cells thick over itself, up to 23% of a
  part's triangles in a first version of the stock mantis, and a folded triangle would be culled from outside and
  leave a hole. A chunk with over one folded triangle in 50 keeps its next finer level. Levels still meet, since
  borders match at every level. So thin limbs lose little at lower levels: the stock mantis is over the alien
  budget of 2500 at full detail and keeps two thirds of its triangles at quarter. Budgets have to come from `detail`
  pieces, thicker limbs at a coarser cell, or drawing fewer. M15.2 can show what this costs, and M15.7's gate enforces it.
- **Faces with T-junction repairs** are triangulated by clipping corners and not by a fan, since a fan makes
  triangles of no area and leaves pinholes where neighbours do not share the repaired points.
- **Not linked by the game yet,** so reading and building add nothing to its binary. M15.4 measures that.

`cargo run --release -p mscent-bench --example model_bench`, best of five, dev machine:

| Model | File | Build | Estimate | Triangles at full, half, quarter |
|---|---|---|---|---|
| Sentry, 6 solids, a cut that only reaches its lens | 675 B | 0.5 ms | 1.4 ms | 556, 256, 148 |
| Dart, 7 solids through a local CSG | 877 B | 5.7 ms | 11.4 ms | 908, 408, 300 |
| Mantis, five parts | 1.2 KB | 4.6 ms | 6.7 ms | 3180, 2516, 2130 |
| 63 solids, direct | 2 KB | 0.38 ms | 0.6 ms | 756 at every level |
| 8 solids, 4 cuts | 433 B | 7.7 ms | 13 ms | 228 |
| 16 solids, 8 cuts | 798 B | 26 ms | 52 ms | 484 |
| 32 solids, 16 cuts | 1.5 KB | 110 ms | 207 ms | 996 |
| 48 solids, 16 cuts | | refused | 369 ms | |
| Ball bitten 60 times at cell 0.25, smoothed twice | 4.4 KB | 62 ms | 149 ms | 52 020, 31 948, 26 344 |
| The same, smoothed 8 times | | refused | 314 ms | |

The estimate stayed 1.3 to 3.2 times above the build on every row, and a local CSG costs less than the
spike's whole hull only because it holds fewer brushes: it grows with their square, so plans should keep cuts to a
few solids. A file made to be slow is refused after reading alone, which takes a few hundred
microseconds at most on these files. A level's worth of 12 such kinds at
5 to 10 ms is about 100 ms here, and an estimated half a second on the i3-7100U, not measured there.

### M15.2

`crates/mscent-render/src/model/`: `ModelTable`, and `ModelInstance`s handed to a frame through
`Effects::models`. The fill and the shading are the voxel path's, in a model's own space: its eye, light direction,
texture axes and corner positions are all in model units, and only screen positions and vertex light are worked out in
the world. So the highlight, the bump and the blended seams needed no changes, and the voxel path now takes its
scene and a transform for near-plane clipping as arguments.

`cargo run --release -p mscent-render --example model_shot`, best of 40, one worker, 640x360, about 8 world units
away, in a room with no other cost than its walls. Dev machine, with noise of a few hundredths of a millisecond:

| Model | Triangles | Adds at Low | Adds at High | Flat path, same mesh, empty frame |
|---|---|---|---|---|
| Sentry | 556 | 0.11 ms | 0.17 ms | 0.11 ms |
| Dart | 908 | 0.12 ms | 0.14 ms | 0.19 ms |
| Mantis | 3180 | 0.36 ms | 0.45 ms | 0.46 ms |
| 12 mixed | 13 836 | 1.49 ms | 1.74 ms | not run |

So a textured model costs about what the flat path does, since drawing it first spares the walls behind it their
shading. The "adds" column is net of that, and a model drawn against an empty frame would cost more. The room alone
takes 4.3 and 8.4 ms. An estimated factor of 5 puts the 12 at about 9 ms at High on the i3-7100U, which is a lot of a
16.6 ms frame; not measured there.

- **Binary:** the draw path is linked now, as a frame can reach it: 2.03 to 2.05 MB, 93% of the budget.
- **Allocations:** 1380 a frame with the three designs and 1379 without (`tests/model_alloc.rs`).
- **Texture rides on the body:** moving or turning a model with the camera leaves the picture alone, to within the
  1/16 pixel snap of its edges (`a_texture_rides_on_a_model_that_moves`, `..._turns`). Both fail when the texture is
  mapped in the world, which was tried.
- **No pinholes** from 40 random sides and distances, so through every level of detail. The picture is the same for any
  number of bands (`tests/model_frame.rs`).
- **The budget is not a hard limit.** Past `MAX_MODEL_TRIS` the farthest models take their lowest level and none is left
  out, so a crowd can pass it, as the 12 do: leaving an enemy out of the picture would be worse.
- **Models are not culled by portals.** A model behind a wall is still projected and lit, and the z-buffer hides it. The
  old actors work the same way. Worth doing when M15.4 has real rooms to measure in.
- **Light is a stopgap until M15.3:** the old per-vertex lamp sum, in grey, through walls, with one direction per model
  for its bump and highlight. Parts do not move until M15.5: `Motion` is read and kept, and nothing applies it.

## Goal

Today every enemy is one mesh: a red ball with two horns (`Mesh::enemy`, 264 triangles, flat colour),
and the green ball for a friendly. This design adds a pipeline that turns a seed into drones, ships and
aliens that look different from each other, using the three things the engine is already good at:

- **Brushes** for hard surfaces: flat panels, sharp edges, wings, blades. Surface nets round these off.
- **Voxels** for anything that grows: bodies, limbs, pits, blends. Convex brushes cannot do these.
- **Texture Studio recipes** for what they are made of.

## The design in brief

| Question | Decision | Why |
|---|---|---|
| Where does variety come from? | Five layers: body plan, discrete genes, modules, skin, selection | Numeric ranges alone give siblings, not variety; the spike showed this ([below](#what-the-spike-showed)) |
| Brush or voxel? | Both in one model, chosen per piece | The mantis's brush blades on a voxel body read better than either alone, with no visible seam |
| What ships? | A **blueprint**: a small text file listing pieces, in the manner of a `.mst` recipe | The game already links CSG and the mesher, and generators stay out of the 2.2 MB binary |
| Where do generators live? | A new editor-side crate, `mscent-foundry` | The game reads the pieces a seed produced, never the seed, so libm differences cannot reach it |
| How are hulls meshed? | Each solid directly from `brush_faces`; only a solid that a cut touches goes through CSG | Direct took about 0.02 ms where the BSP took up to 163 ms, and gave under half the triangles |
| Voxel cell | Per part, from its thinnest radius | Cell 1.0 costs a quarter of the triangles, but breaks thin limbs into beads |
| How is it drawn? | A new model path built on the voxel triangle fill, in model space | Textures must ride on a moving body, and `draw_polygon`'s path is the slow one |
| How does it move? | Rigid parts about pivots; a per-vertex sway for limbs | No skeleton, skinning or new data per frame |
| What does the simulation see? | Hit spheres written in the blueprint | Replay never depends on a mesh, and the mesh can change without changing play |
| Who picks the winners? | Gates and a diversity archive narrow thousands of seeds to a contact sheet; a person promotes | The project already reviews pictures, not numbers |

## The pipeline

```text
                                        editor side (mscent-foundry)
  seed --> body plan --> genes --> sockets + modules --> pieces
                                                           |
                                                           v
                          build --> gates --> silhouette --> archive --> contact sheet
                                                                             |  a person promotes
  ---------------------------------------------------------------------------|---------------------
                                        shipped data                         v
                 assets/enemies/enemies.txt + name.msb + assets/textures/name.mst
                                                           |
                                        game at load       v
                 parse, check cost --> build parts + LOD --> ModelTable --> model path in each frame
```

The build step is the same code in both halves, so what the foundry judged is what the game draws.

## Variety in five layers

Each layer multiplies the one above it. They are ordered by how much they change what a player sees at
a distance, and effort should go in that order.

### 1. Body plans: different code, not different numbers

A body plan is a function from seed to pieces, one file each, as the voxel generators are. Two plans
share nothing but helpers, so they differ in topology. Three per class are spiked, and the rest are the
target for a first set of eighteen.

| Class | Plan | Shape | Brush | Voxel | What moves | Role it signals |
|---|---|---|---|---|---|---|
| Drone | Sentry\* | Faceted hub, pods on struts around it, one lens | Most | Lens | Pods spin | Holds and shoots |
| Drone | Hornet\* | Diamond dart with swept blades in 1 to 3 pairs | Most | Canopy | Blades flutter | Fast and fragile |
| Drone | Urchin\* | Pitted ball of radial spikes | Spikes, or none | Body | Spikes pulse | Comes at you |
| Drone | Gyro | Nested rings around a core | Rings | Core | Rings on separate axes | Shielded core |
| Drone | Swarmlet | Tiny tetra-frame, thrown in dozens | All | None | Whole body tumbles | Weak alone |
| Drone | Crawler | Flat disc on hooked legs, hugs walls | Disc | Legs | Legs swing | Ambush |
| Ship | Dart\* | Spine with one of four wings and one of three engine layouts | Most | Canopy | Flaps | Strafing runs |
| Ship | Hammer\* | Beam with a crossbar head, nacelles, greebles | Most | Bridge | Nacelles swivel | Heavy gunship |
| Ship | Manta\* | Living hull, ribs, metal nozzles grafted on | Nozzles | Hull | Ribs sway | Carrier |
| Ship | Ring | Open torus around an engine | Torus facets | None | Ring spins | Flies through gaps |
| Ship | Barge | Stack of boxes, turrets in sockets | All | None | Turrets track | Slow and armoured |
| Ship | Wreck | A ship plan with bites carved and a growth over it | Remains | Growth | Growth sways | Infested variant |
| Alien | Squid\* | Spheroid head, arms from a random walk, eyes in pits | None | All | Arms sway | Chaser |
| Alien | Mantis\* | Segmented body, legs, ridge, brush blades | Blades | Body | Blades swing | Melee |
| Alien | Polyp\* | Hollow cap with branching fronds beneath | None | All | Fronds sway | Drifts and stings |
| Alien | Hive mother | Swollen sac with hex pits and small grafted limbs | None | All | Sac pulses | Spawner |
| Alien | Eel | Long chain of shrinking rings, jaw carved in | Teeth | Body | Sway along body | Fast in tunnels |
| Alien | Crab | Wide plates over soft legs, brush-cut shell | Shell | Legs | Legs swing | Armoured front |

\* Spiked. The roles are what a silhouette should promise; see [Play](#play).

### 2. Genes: discrete choices first

A gene is one choice a plan draws from its seed. **The first draw is an arrangement, not a size.** The
spike's first dart drew span, sweep and length from ranges and gave four nearly identical jets. Changing
it to four wing layouts, three engine layouts, two body lengths and four cross-sections, with no more
code than that, gave four aircraft that are visibly not the same.

| Kind | Examples | Effect |
|---|---|---|
| Arrangement | Wing style, engine layout, limb attachment | Changes the silhouette |
| Count | Arms 4 to 9, pods 2 to 5, segments 3 to 6 | Changes the rhythm |
| Symmetry | Bilateral, radial n, slipped, or broken on purpose | Reads as machine, creature or wreck |
| Proportion | Taken from low, middle or high, never uniform | Avoids the average of everything |
| Cross-section | 3, 4, 6 or 8 sides on brush prisms | Changes how light breaks on it |
| Material | Which of the faction's skins goes where | Changes how it reads at a distance |

### 3. Modules: sockets shared across plans

A plan marks **sockets** with a tag, a place and a direction: `engine`, `weapon`, `sensor`, `limb`,
`crest`. A module is a small plan that fills one. Eight of each tag mixed across eighteen plans is most
of the combinatorics, and it is what lets a hornet carry a mantis blade or a dart have a squid's eyes.
An alien module on a ship socket is how a "grafted" faction happens without a plan of its own.

### 4. Skin: Texture Studio

The spike wrote four new materials in 14 lines of recipe or fewer each: plating, chitin, flesh and glass.
They loaded through `MaterialTable::load_dir` with nothing else changed, and chitin's cell edges alone
turn a voxel body into plated armour. Three levels of skin variety, cheapest first:

1. **Instance tint.** A colour multiplied into the vertex light. It costs nothing per pixel, and gives
   wounded, elite and faction variants of one blueprint.
2. **Skin sets.** A faction is three or four materials, used by role: `hull`, `trim`, `soft`, `glow`.
   A blueprint names roles, and `enemies.txt` binds them to materials, so one blueprint can wear any set.
3. **Recipe variants.** The foundry writes a new recipe from a template by changing its palette, panel
   count or cell count. Stock recipes average about 4 ms at 128 px, so this stays cheap, but it counts
   against the directory's limits of 64 tiles and 30 s, and material ids are append-only and limited to
   254 on voxels. Keep to about 24 enemy materials and let tint do the rest.

Marks belong in the volume: voxel `Paint` dabs for bands and eyes, and the recipe's `text` operator for
hull numbers. A faction mark or a hazard band does more for identity than any amount of noise.

Glow needs one engine change. Only `lightpanel` on a brush face draws emissive, by id. A model's material
gets a `glow` flag in its blueprint, drawn flat with no light, so eyes and engines stay bright in a dark room.

### 5. Selection: throw most of it away

A plan run over 2000 seeds gives mostly duplicates and a few failures. The foundry keeps what is useful.

**Gates** reject outright:

| Gate | Test | Why |
|---|---|---|
| One object | `solid_parts` is 1 for each part, and parts touch | Beads and floating bits read as bugs (the spike's cell 1.0 limbs fail this) |
| Triangles | Under the class budget at full detail | The spike's polyps span 1984 to 4164 |
| Bounds | Fits the class's box, and its hit spheres cover at least 80% of its silhouette | What you see is what you hit |
| Legible | Silhouette fills enough of its box at the distance enemies are fought | A thin wire of a thing cannot be aimed at |

**The archive** then keeps variety, in the manner of MAP-Elites. Each survivor is rendered as a 32x32
silhouette from six directions, which gives elongation, spikiness (perimeter squared over area), holes
and the hard-surface share of its triangles. Those pick a bin, 4 x 4 x 3 to start with, and a new design
replaces a bin's holder only if it is more legible. One whose silhouettes overlap a kept design by over
80% in the same bin is dropped. Bin counts and thresholds are starting values for M15.6 to tune.

**The contact sheet** shows what survived, with its class, seed and triangle count, through the same
shader as the game and at the distance it will be fought. A person promotes a design, which writes its
`.msb` and an `enemies.txt` row. An agent can run all of this headless, as with `--bake-texture`:

```
cargo run --release -p mscent-editor -- --foundry ship --seeds 2000 --out target/foundry
```

## Data

### Blueprint

Text and diffable, like a recipe. Pieces are the numbers a generator produced, so a file also reads as a
record of the design, and can be edited by hand.

```text
miniscent-blueprint 1
name mantis-07
class alien
plan mantis 7
scale 0.25
skin hull chitin
skin blade metal
glow eye

part body cell=1
add spheroid 6 0 0  1 0 0  2.8 2 hull
add round_cone 6 2 0  8 4.5 1  0.8 0.6 hull
carve sphere 7.5 1.4 1.5 0.6
smooth sphere 0 0 0 9

part blade-l pivot=8,4.5,1 axis=0,0,1 motion=swing:0.6:2
solid plate 0.5  7.25 4.5 1  10 4.5 3  18 4.5 0  8.5 4.5 -1 blade

hit 6 0 0 2.2
hit 13 4.5 1 1.0
hit 13 -4.5 1 1.0
```

- **Model units.** One unit is a quarter of a world unit (`scale`), so the snap grid of 1/8 is 1/32 of a
  world unit, and cell sizes of 0.5 and 1 give fine enough surfaces. At world scale, the 1/8 grid and
  cell were coarse for a thing two units wide.
- **`solid`** is a convex prism, plate or box, stored by shape and not by planes. **`cut`** subtracts from
  the solids of its part. A `detail` flag on a piece drops it at lower levels of detail.
- **`part`** is a rigid piece with its own cell size, pivot and motion. Its mesh is built alone.
- **`hit`** spheres go into the simulation. They are written by the foundry, fitted from the silhouette,
  and never recomputed at load.

The table of kinds is append-only, as `materials.txt` is, because levels store a `kind` number:

```text
# assets/enemies/enemies.txt: id name blueprint hp speed sight attack damage cooldown
2 hornet-03 hornet-03 40 9 40 8 4 0.4
```

Kinds 0 and 1 stay the two that exist, drawn as they are now, so levels already made do not change by a
pixel. An unknown kind still uses the basic drone, as `stats_for` does now. `Entity::Enemy`'s `u8` allows
253 more kinds, and `defineEnemy:base:` works with them as it does now.

### Hostile input

A blueprint is untrusted, like a level or a recipe, and building one does real work.

- Limits on the file (64 KiB), parts (16), pieces (512), brushes in a part with cuts (64), and chunks (256).
- Every number goes through the existing checks: `Prim`'s constructors and `Brush::new` return an error for
  anything out of range, and a part may not name a cell size the lattice lacks.
- A model is refused **before any work** when its estimated cost passes a limit, by the rule recipes use:
  costs depend only on the file, never a clock, so a file gets the same answer everywhere.
- A model that passes but makes over `MAX_VERTICES` in a part is refused. Indices are `u16`.
- Malformed files are load errors naming the file. They do not silently become drones.
- Same tests as `voxel/wire_tests.rs`: bit flips, truncation, and each limit.

### Code map

```text
crates/mscent-foundry/           plans/*, modules/*, genes, sockets, gates, silhouette, archive (new crate:
                                 math, world; never linked by the game; recorded in AGENTS.md)
crates/mscent-world/src/model/   blueprint (read, write), build (pieces to mesh), table, cost
crates/mscent-render/src/model/  list (LOD, projection), shader, light; ActorKind gains Model(id)
crates/mscent-game/src/enemy.rs  kinds table, facing, hit spheres
apps/editor/src/ui/foundry/      workspace beside the Texture Studio; --foundry headless
```

## Building a model

**Solids go directly.** A part's solids each become faces through `brush_faces`, with no BSP. Overlaps are
left to the z-buffer and back-face culling. Hidden faces cost a little overdraw and nothing else.

**Cuts go through CSG, locally.** Only solids that a `cut` touches go to `compile` and `extract_faces`,
with the cuts and a surrounding subtract brush whose faces are dropped. The world starts solid, so that
brush is what gives the model an outside. A part with many cuts costs real time, so plans should carve
detail with voxels or leave it to the texture, and the limit above bounds the worst case.

**Voxels go through the usual path.** Each part with soft pieces gets an `Add` volume, with its pieces applied
by `apply_dab` and meshed by `Mesher`, then two levels by `clustered`. A cut also cuts these, since an `Add`
volume resolves later subtract brushes. `Volume` keeps its box limits and needs nothing new.

**Cell size per part** comes from the thinnest radius in the part, at no less than 1.5 cells, the rule
`Plan` already uses. A squid's head meshes at 1.0 and its arms at 0.5. So limbs do not break, and the body
does not pay for them.

**The result** is one format: positions, normals, triangles, material per triangle. Soft parts share vertices and
so shade smoothly. Solid faces have vertices of their own and so keep their hard edges.

## Drawing

### Model path

`list_mesh` and `fill_meshes` draw flat colour through `PolyList`, allocate per actor, and only know yaw.
A new path sits beside them. It leaves stock meshes alone, so levels using them draw the same pixels.

- **Pose** becomes rotation, position and scale, using `Quat`. Enemies point where they fly, in 6DoF.
- **Frame preparation:** as `VoxelList` does for chunks, each visible part's vertices are transformed and
  projected once, before tile bands start, and bands share them read-only. No per-frame allocation, held
  by `tests/frame_alloc.rs`.
- **Triangle fill** is the voxel path's: fixed-point back-face rejection and top-left rule.
- **Texturing** uses the voxel path's dominant-axis projection, but of **model** positions and normals, so
  a texture rides on the body. Axis blending costs what it costs on voxels and is kept only at full detail.
- **Levels of detail** go by screen size: voxel levels from `clustered`, and `detail` pieces dropped.
- **Budget:** `MAX_MODEL_TRIS`, 8000 submitted per frame to start with, nearest first. Past it, the rest take
  their lowest level, as the voxel governor does, so it can be passed by a crowd.

### Light

Vertex light today adds up every lamp in the level for every vertex, with no shadows, so a monster in a
dark room can be lit from behind a wall. Instead, per instance, once a frame:

1. Lamps pass the same test dynamic lights do: `reaches_leaf` of the leaf the model is in.
2. They, the frame's dynamic lights and the ambient are gathered into a six-sided ambient cube and one
   dominant direction.
3. Each vertex reads the cube with its normal, at about one dot product a vertex and no lamp loop. Bump and
   the highlight use the dominant direction, moved into model space once.

So a model darkens as it goes into shadow by room, not by surface. No ray is traced while playing, which
keeps the promise in [lightmaps.md](lightmaps.md). Its limit: there is no shadow inside a leaf. Baked
samples could replace step 1 later.

### Motion

| Motion | How | Used by |
|---|---|---|
| Whole body | Facing `Quat`, eased toward velocity or target | All |
| Rigid part | Spin or swing about a pivot; a part's matrix per frame | Pods, rings, flaps, blades, turrets |
| Sway | Each vertex moves by a sine of its weight, from distance to the part's root | Arms, fronds, ribs |
| Damage | A second mesh per part with `wound` pieces carved and charred, chosen by health | Bosses only, since it doubles the part's load cost |
| Death | Parts fly apart as debris for a second, with the existing `shatter` | All |

Motion and debris take time from the tick and nothing from the simulation's state, so replay is untouched.

## Play

The enemy simulation stays as it is, with three additions.

- **Facing:** the `Enemy` gains a `Quat` eased each step. Today's yaw is worked out while drawing.
- **Hit spheres:** `Enemy::sphere` becomes a short list in model space, turned by facing. `hitscan` takes a
  list already. Up to eight an enemy, tested only when the body's sphere is hit first.
- **Body:** the sphere for world collision is a bounding one. So a long ship cannot enter a small door, and
  no nose pokes through a wall. This is a level design rule as much as a limit; the editor warns when a
  spawn cannot fit.

Silhouette should promise behaviour: a player learns that a spiked ball rushes and a hammerhead sits and shoots.
Each plan sets default stats to match, and `EnemyStats` already covers them. **New behaviours, flocking, and
turrets that aim on their own are out of scope**; this design gives them something to attach to.

Within one kind, an instance chooses among up to four variants and a tint by the seeded `Rng` every enemy
already has, so a crowd is not clones, and replays match.

## What the spike showed

The spike built each model into a real level with `Level`, `Brush`, `Volume` and `LevelRenderer`, and took its
triangle counts from `Mesher`, `clustered` and `extract_faces`. Nine plans, four seeds each, in model units.
Draw time is the existing `draw_mesh` with flat colour at 640x360, about 7 world units away, best of 12.
`Mesh::enemy` on the same test: 264 triangles, 0.05 ms, 4099 pixels.

| Plan | Pieces | Voxel tris, cell 0.5 | Voxel tris, cell 1 | Half detail, cell 1 | All tris via BSP, cell 1 | Draw ms, cell 1 |
|---|---|---|---|---|---|---|
| Sentry | 9-13 | 412-540 | 84-108 | 12-36 | 334-524 | 0.04-0.07 |
| Hornet | 9-11 | 148-184 | 28-40 | 4-12 | 260-346 | 0.05 |
| Urchin | 16-37 | 2104-3940 | 536-880 | 178-348 | 712-880 | 0.09-0.12 |
| Dart | 11-12 | 636-660 | 156-172 | 36-48 | 400-538 | 0.09-0.18 |
| Hammer | 49-56 | 340-380 | 80-100 | 20-40 | 1508-1848 | 0.23-0.29 |
| Manta | 23-35 | 11164-16436 | 2612-3828 | 998-1362 | 2764-3980 | 0.28-0.39 |
| Squid | 46-60 | 4776-5808 | 1016-1292 | 302-400 | 1016-1292 | 0.09-0.13 |
| Mantis | 21-31 | 4272-8168 | 976-1940 | 340-634 | 1000-1964 | 0.10-0.24 |
| Polyp | 47-128 | 9132-18992 | 1984-4164 | 758-1754 | 1984-4164 | 0.26-0.46 |

Build costs for the same models:

| Step | Result |
|---|---|
| Dabs at cell 1 | 0.03 to 6.8 ms a model |
| Meshing with two levels of detail, at cell 1, empty chunks included | 2 to 5.5 ms a model |
| Same at cell 0.5 | 8 to 18 ms a model |
| Hulls through `compile` and `extract_faces`: Hammer | 104 to 163 ms, 1408 to 1768 triangles |
| Hulls through `compile` and `extract_faces`: Dart | 31 to 67 ms, 228 to 366 triangles |
| Same solids through `brush_faces`: Hammer | 0.014 to 0.022 ms, 552 to 656 triangles |
| Same solids through `brush_faces`: Dart | 0.004 to 0.006 ms, 160 to 188 triangles |

What this says:

- **Brushes and voxels join with no work.** The mantis's blades and the urchin's spikes meet voxel bodies with
  no seam to see, and no code for it. It costs nothing beyond what `Add` volumes already do.
- **Hulls should skip the BSP.** T-junction repair and splitting give about 2 to 2.7 times the triangles of plain
  solids, at thousands of times the cost. The direct numbers leave out cuts, so they are the floor for a
  model that has them, and the cost of a local CSG on a few solids is not measured.
- **Cell 1 is right for bodies, wrong for limbs.** It takes about a quarter of the triangles and looks as good
  on a body. Squid arms and polyp fronds thinner than two cells broke into separate beads. That is the reason
  for cells per part. The spike applied dabs directly and skipped the widening `Plan` does.
- **Load cost is fine if hulls are direct.** The worst model at cell 1 is under 15 ms on this machine, by adding the
  rows above. Twelve kinds in a level is about 100 ms, against 47 ms declared for all stock textures. An estimated
  factor of 5 on the i3-7100U gives half a second. Not measured there.
- **Budgets work out with gates.** Starting budgets at full detail are drone 800, ship 2000 and alien 2500 triangles,
  which most of the spike's seeds meet at cell 1. Manta and Polyp need fewer ribs or fronds, and the gate does that.

What it does not say:

- **The textured path was unmeasured.** Flat fill is what the table times. [M15.2](#m152) has measured the real one.
- **The spike's models are static.** They were lit as level geometry, baked with shadows, which flatters them. The
  ambient cube will look flatter, and M15.3 compares them side by side.
- **Silhouette selection did not run.** All sheets show the first four seeds, ungated.
- **Some plans are weak.** The polyp's gills cannot be seen under its cap, the squid's eyes clump, and the manta's ribs
  are lost against a large body. Plans need a person's eye like any other art.

## Editor

The **Foundry** is a workspace opened from a menu, beside the Texture Studio, and built the same way: its own document
and undo, a worker with cancel, and the level untouched.

```text
+-- kinds ------+------- sheet of candidates -------+-- genes -----------+
| drone         | [ ][ ][ ][ ][ ][ ]                | plan     [hornet]  |
|   hornet-03   | [ ][x][ ][ ][ ][ ]                | wings    [3 pairs] |
| ship          | [ ][ ][ ][ ][ ][ ]                | symmetry [mirror]  |
| alien         +-----------------------------------+ skin     [wasp]    |
| + New         | orbiting view, lit as in the game | seed [1187] [roll] |
| Promote       | tris, hit spheres, part motion    | Open as level      |
+---------------+-----------------------------------+--------------------+
```

- **Roll** runs a plan over a batch and fills the sheet. Clicking a candidate shows it orbiting, moving, and
  at fighting distance. Locking a gene and rolling again breeds around it.
- **Open as level** makes a temporary level from a blueprint: brushes and volumes in an empty room. So every sculpting
  tool works, as do Texture Studio and Instant Play with no new code. The spike did just this to get its pictures.
  Bringing a sculpted result back into a blueprint needs a format for deltas, which is [question 2](#open-questions).
- **Placing:** the enemy tool's panel lists kinds by name, with a thumbnail. The 3D view draws the model at its marker.

## Glitches designed out

| Glitch | Prevented by | Test that reads real output |
|---|---|---|
| Texture swims on a moving enemy | Model-space projection | Texels under a tracked point stay equal over a turn |
| A part drifts off its body | Parts touch at their pivot, by gate | Silhouette of each pose is one connected piece |
| Beaded limbs | Cell per part, and the gate on `solid_parts` | Every promoted blueprint passes in CI |
| Enemy lit through a wall | Lamps tested by leaf | Pixel test: model behind a wall from a lamp is at ambient |
| Eyes go dark in a dark room | `glow` material | Pixel test at zero lamps |
| Bullet goes through a visible wing | Hit spheres gated on silhouette cover | Rays at 1000 silhouette pixels: at least 80% hit |
| Nose through a wall | Bounding body sphere | Extended `tests/fuzz.rs` with large radii |
| Replay changes with a mesh | Simulation reads only `hit` lines | Replay bit-exact after a blueprint's pieces change |
| Stock levels change | Kinds 0 and 1 keep their path | Existing snapshots unchanged |
| A hostile file stalls load | Cost refused before work | Forged files refused in under 1 ms |
| Over budget when crowded | `MAX_MODEL_TRIS` and lowest levels | `past_the_budget_the_farthest_take_the_lowest_level`: the nearest at full detail, the farthest at the lowest, and none left out |

## Work items

Each leaves the game and editor working, and sizes are as in work-items.md.
[work-items.md](work-items.md) holds the same table.

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M15.1 | `mscent-world::model`: blueprint reader and writer, limits, cost, build with direct solids, local cuts, cells per part | | L | Round trip; bit-flip and truncation fuzz; same bytes give the same mesh; `model_bench` repeats the build table here, including local cuts; binary size against budget |
| M15.2 | `mscent-render::model`: `Quat` pose, frame preparation, fill, model-space shading, levels, budget, table | M15.1 | L | Texture does not swim; `frame_alloc` unchanged; textured cost per model recorded; side-by-side with flat path |
| M15.3 | Light: leaf test, ambient cube, `glow`, instance tint | M15.2 | M | Wall and zero-lamp pixel tests; compared by eye with the baked picture; `check_frame_cost.py` gains a crowd scene |
| M15.4 | Game: `enemies.txt`, kinds, facing, hit spheres, bounding body; level and script use by name | M15.1 | M | Replay bit-exact; fuzz with large radii; old levels draw the same pixels; a hand-written blueprint plays end to end |
| M15.5 | Motion: parts, sway, debris on death | M15.2 | M | Replay unaffected; moving parts at both ends of a swing by pixel test; no per-frame allocation |
| M15.6 | `mscent-foundry`: helpers, genes, sockets, gates, silhouette, archive; `--foundry` headless | M15.1 | L | Same seed gives the same file; a run of 2000 seeds writes a contact sheet; gates reject planted failures |
| M15.7 | First plans: the nine spiked, with two weak plans reworked | M15.6 | L | Owner reviews a sheet of each; no two kept designs of a plan overlap by over 80% |
| M15.8 | Skin sets: plating, chitin, flesh, glass and two factions as recipes | M15.3 | M | Directory limits still met; load time and binary size recorded; ids append-only |
| M15.9 | Modules and the other nine plans | M15.7 | L | Eight modules of each tag; a sheet shows a module on three plans of different classes |
| M15.10 | Editor: Foundry workspace, placing by name, Open as level | M15.6, M15.4 | L | `--selftest` rolls, locks a gene and promotes; worst UI tick during a roll held by `tick_costs` |

The first playable result is M15.1 to M15.4 with one hand-written blueprint per class. M15.6 onward is what makes
it many. M15.1, M15.2 and M15.6 can run in parallel once the format is fixed.

## Alternatives rejected

| Alternative | Why not |
|---|---|
| Voxels only | Surface nets round every edge, so a machine reads as melted, as the design for volumes says itself |
| Brushes only | Convex pieces make no creature, and no blend between a limb and a body |
| Whole hull through CSG | Up to 163 ms a model and over twice the triangles, for cuts most models barely use |
| Compile a model into the level's BSP | Enemies move. Movers have this geometry, but one per enemy is out of reach |
| Ship baked meshes | Hundreds of KB per design against an estimated few KB of pieces, where the game already links the builder. A cache can be added if load time asks for one |
| Generators in the game, a new enemy per run | Costs binary budget (2.02 of 2.2 MB at last count), and untested designs would reach the player. A later mode could bake in the same way |
| Skeletal animation | Skinning per vertex for a game of flying things; rigid parts and sway cover most of what is seen |
| Destructible enemies at runtime | Meshing on the tick is budgeted for one dab at a time, and only for chunks in a level. A precomputed wound mesh gives the look |
| Noise on the parameters as the source of variety | The spike's first dart: four nearly identical planes |

## Open questions

1. **How many on screen?** I took about 12 near and 64 at most. A wave game would pull the budgets down.
2. **Should sculpted models ship?** I took no: blueprints hold pieces, and Open as level is for trying things. Hand-sculpted
   heroes and bosses would need a `Volume` per part, stored by the level format's section 5 at tens of KB. It is a
   small addition if wanted.
3. **Do enemies leave their rooms?** A bounding sphere means big ships stay in big spaces. This suits
   [viewports.md](viewports.md)'s larger spaces, but levels need room for them.
4. **New kinds and the game's `Entity::Enemy` number.** Levels hold a number, so a kinds table has to be kept with a level,
   the same limit [texture-studio.md](texture-studio.md) lists for material ids. Saving names in levels is a format change, left for later.
5. **Reference machine.** As everywhere else, the i3-7100U is unmeasured, and budgets rest on the estimated factor of 5.
