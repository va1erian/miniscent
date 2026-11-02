# Voxel volumes: design

Status: M11.1 and M11.2 are done (field, dabs, meshing, level of detail, connectivity, level file); the game and editor load and keep volumes but do not draw or edit them yet. Work items are [M11](work-items.md#m11-voxel-volumes).

Brushes are good at architecture and bad at anything that grows. This design adds a second kind of
geometry for hives, coral, mushrooms, trees, cave systems, stalactites and alien interiors, painted
in the editor in the manner of CryEngine 2's voxel tool, and damageable in the game.

## The design in brief

| Question | Decision | Why |
|---|---|---|
| How does it fit CSG? | A **voxel volume is an entry in the ordered brush list**. Earlier brushes are what it starts from, later brushes cut it | Keeps the one rule the editor already teaches: list order decides |
| Add and subtract? | An `Add` volume only adds material. A `Subtract` volume also carves its box out of the BSP, then its field owns that box | Cave entrances need to dig into brush solid; coral on a floor does not |
| What is stored? | Sparse chunks of **signed deltas over the brushes**, not final density | A brush moved next to a cave updates the cave; sculpting survives |
| Surface | **Surface nets**, one mesh per 16³ chunk | No tables or ambiguous cases, about half marching cubes' triangles, and chunk borders match by construction |
| Drawing | A **new indexed triangle path** beside `draw_polygon` | Measured: about 4x faster on the same triangles, while also texturing and lighting them |
| Light | **Baked per surface cell**, interpolated across triangles; no atlas | Thousands of tiny faces cannot each own a lightmap rectangle |
| Collision | Exact swept sphere against the **same triangles that are drawn**, merged with the BSP result | What you see is what you hit; reuses the existing edge and vertex contact maths |
| Destruction | Subtract-only, inside the box, applied on the fixed step | Cannot trap an entity, open the world to the sky or break replay |
| Editing | Every dab copies only the chunks it touches; a live path re-meshes them without running CSG | Today's preview per edit takes 15 to 706 ms; a dab takes microseconds |

The world's solidity becomes one sentence: **a point is solid if the BSP says so or a voxel field
says so.** Everything else follows from it.

## What each use case needs

| Use case | Volume | Tools |
|---|---|---|
| Stalagmites, stalactites | `Add` | Drip generator, Add, Roughen |
| Mushrooms, coral, trees | `Add` | Growth generators with seed and ghost preview, Smooth, material paint |
| Hive interior | `Subtract` | Comb generator, Smooth; carved from rock, wax walls left behind |
| Cave entrance in a brush wall | `Subtract` | Carve starting on the wall; the volume appears by itself |
| Cave systems | `Subtract` | Tunnel path, Carve, Smooth, Noise |
| Alien interiors | `Subtract` | Field fill (warped gyroid or noise) inside the box, then sculpt |
| Weathering, wear | either | Roughen, Smooth, material paint |
| Destructible rock | either | `destructible` flag on the material; the weapon carves |

Not goals: replacing brushes for architecture (surface nets rounds hard edges; that is their job),
voxels as moving objects, physics-driven falling rubble, and an unbounded streaming world.

## Core model

### A step in the ordered CSG

`Level` gains `volumes`, each with an `order` drawn from the same sequence as brushes. In the editor
a volume is a `BrushDef` with a new `Shape::Volume`, so it gets selection, list reordering, the
add/subtract toggle, move, undo and the props panel for little cost (`Shape` has 17 use sites in the
editor to cover). Slice, Vertex and Carve refuse a volume with a message, as they already refuse
other shapes.

For sample `p` of a volume at order `k`:

```text
base(p)     = solid-start fold of brushes with order < k   (Subtract volume)
            = air                                          (Add volume)
edited(p)   = clamp(base(p) + delta(p))
resolved(p) = edited(p) cut by subtract brushes with order > k
```

`fold` applies brushes in order using each convex brush's signed distance, the maximum of its plane
distances: `Add` takes the union, `Subtract` the difference. Only brushes overlapping the box count,
and a chunk that no brush plane crosses skips the per-sample work. The BSP compile receives one extra
subtract brush for each `Subtract` volume, at its `k`, and nothing for an `Add` volume.

Later add brushes are left out on purpose. The BSP already holds them, so the rule "BSP or field" stays
correct, and a field that followed them would lay a surface exactly on their faces and fight them.

This gives the behaviour a level designer expects, with no special cases:

- Move a room's wall under a cave volume: `base` changes, untouched rock follows, carved tunnels stay.
- Cut a door brush through finished rock, later in the list: `resolved` loses the rock there.
- Move a `Subtract` volume: its sculpting moves with it and its walls re-form from what lies there.

Volumes may touch but not overlap. A move or growth that would overlap is refused, like a Vertex
move that loses a corner. Touching volumes must read each other's resolved samples across the border.
**Not yet:** today a volume's ring sees only brushes, so a tunnel across a shared face is sealed by a
wall on each side. M11.3 does this before volumes can be seen in a level.

A mover brush stays out of the CSG, so M11.3 builds a `Context` from `Level::static_brushes`.

`--dump-level` refuses a level with volumes, because the text form has none and packing it again
would lose them. A text form can follow if an agent needs to write volumes.

A volume may not share an order with a brush, because it would then be neither before nor after it.
The loader refuses that. Until a volume is a `BrushDef` (M11.7), the editor keeps each one right after
the last surviving brush that preceded it when loaded, and renumbers brushes and volumes as one
sequence on save.

**Materials of untouched rock.** `base` gives a sample's density only, so a chunk nobody painted says
material 0. Carving into a `Subtract` volume therefore shows material 0 instead of the brush's, until
M11.3 takes the material of the brush that decided the sample.

### Where a Subtract volume meets the BSP

This seam is where a hybrid would show holes. The rules:

```text
   brush solid            |  volume box
                          |
  ========= brush wall ===|......voxel wall continues the same plane
       (drawn by BSP)     |  (drawn by voxels, slides 1/16 cell behind
                          |   the brush wall across the border ring)
         ^ shell face of the box: never drawn, never lit
```

1. **The ring, two rows of samples around the box, is not editable** and is resolved from the brushes
   outside it. A tunnel dug up to the box therefore closes with a voxel surface instead of opening onto a void,
   and a tunnel can never leave the box. Growing the box is the only way further.
2. **Shell faces are never drawn or lit.** The shell brush carries a reserved no-draw material and
   `Compiled::new` drops those faces before lightmap layout. The voxel surface always closes in
   front of them.
3. **The voxel surface runs past the box on every side and is biased 1/16 cell behind brush faces
   there.** Looking along a wall you see the brush face up to its edge and the voxel surface beneath
   it continuing. No gap is possible, and coplanar faces never fight. A planar field meshes exactly
   flat under surface nets, so a wall stays straight through the seam. This takes two rows of ring
   samples: with one, the mesh stops half a cell short of the box on its high sides, because a quad
   needs all four of its cells, and `voxel_shot` showed that gap as a line of holes. A wall lying
   exactly on the first row also needs the second, since the bias turns that row to air.
4. **Openings need no work.** Where ring and interior are both air there is no surface, and the
   BSP's portal from the room into the box's leaf is already correct.

An `Add` volume has none of this: its ring is forced to air, and it intersects brush geometry in the
z-buffer. Triangles wholly inside BSP solid are dropped at meshing, which also saves their light.

### Invariants

Each of these is asserted by a test (see [Glitches](#glitches-designed-out)):

- The surface of a volume is closed across chunk borders: every directed edge is matched by an
  opposite one. An edge may carry four triangles where the surface pinches, which is harmless.
- `resolved` is a pure function of the level's bytes. Same bytes, same mesh, bit for bit.
- With no edits, "BSP with the shell, or field" is solid exactly where the BSP without the volume is,
  at every sample away from a plane.
- Runtime edits only remove material and never touch the ring.

## Data

| Thing | Choice |
|---|---|
| Cell size | Per volume: 0.125, 0.25, 0.5 or 1 world units (powers of two on `SNAP_GRID`). Default 0.5 for caves, 0.25 for growths; a 0.5 cell has a quarter of the triangles for the same surface |
| Sample | 1 LSB = 1/32 cell. `base` and `resolved` are `-63..=63` (±2 cells); `delta` is `i8`, `-126..=126`, enough to override any base |
| Material | A material id per sample, as `u8`: 256 is far more than the game's 5, and no remapping means no out-of-range index to check |
| Chunk | 16³ samples: `Uniform { delta, material }` or `Detail(Arc<[…; 8 KB]>)` |
| Table | `Arc` table of chunks, up to 16³ chunks per volume, so cloning a volume is O(1) |
| World lattice | The box snaps to whole cells; samples are relative to its origin, so moves are by whole cells |

**Copy on write is what the editor relies on.** `Studio` compares and clones the `Document` every
tick from the UI thread, so `PartialEq` on a volume compares `Arc` pointers first and only reads the
bytes of chunks whose pointers differ. An undo step shares every untouched chunk with its neighbours.

Limits: `MAX_VOLUMES` 256, 4096 chunks per volume, `MAX_DETAIL_CHUNKS` 16 384 per level (128 MiB). Tolerances go in `mscent_math::epsilon`.

### File format

Level format version 4 adds section `5`. Versions 1 to 3 still load, with no volumes.

```text
count u32
volume*: order u32, op u8, cell u8, origin i32*3, chunks u16*3, chunk*
chunk:   kind 0 (uniform): delta i8, material u8
         kind 1 (detail):  plane of deltas, plane of materials
plane:   len u32, then len bytes: exactly 4096 is the samples as they are,
         any other multiple of 3 is runs of (count u16, value u8)
```

Noise would triple in size as runs, hence the plain form. Sculpted rock mostly compresses well.

A hostile file is the main threat, and RLE is a decompression bomb by design, so:

- Every count is checked against the bytes left before allocating, as `read_count` does now.
- Each plane must decode to exactly 4096 samples; an empty run or a run past that is an error.
- `MAX_DETAIL_CHUNKS` is charged before each is allocated. Uniform chunks allocate nothing.
- A volume's table is built only once the bytes left could fill it.
- Delta ranges are checked, and overlapping volumes are rejected, as is a volume tied with a brush's order.
- A material id not in the table draws with the existing grey fallback.

`voxel/wire_tests.rs` covers bit flips and truncation (whatever loads is also meshed), forged runs and
each limit. **The writer has no limit of its own:** a level can be sculpted past `MAX_DETAIL_CHUNKS` or
`format::MAX_FILE_BYTES` and then not load, so M11.7 must refuse such edits or fail the save.

Only deltas are stored. As with brushes, the loader recompiles: `resolved` and meshes are rebuilt at
load (about 40 µs a chunk measured for meshing, so a large cave adds tens of milliseconds).

## Surface extraction

**Surface nets**: one vertex in each cell the surface crosses, at the mean of its edge crossings, and
one quad per crossed sample edge. A chunk meshes from a padded block copied from itself and its
neighbours, and owns the quads of the edges whose lower sample it holds.

Border vertices come from the same samples on both sides, so **neighbouring chunks agree exactly with
no stitching step**. Normals come from the field's gradient over the cell's eight corners, so shading
agrees too. A brush's value must also not depend on which chunk asks for it, so its reach is tested per
sample and not only per chunk.

Each mesh stores positions, normals, the cell of each vertex, material per triangle and a **15-bit air
connectivity mask** between chunk faces. M11.3 adds dropping of buried triangles, and M11.6 the per-cell
collision index.

### Level of detail, without cracks

Far chunks use **vertex clustering**: interior cells merge in 2³ (LOD1) or 4³ (LOD2) blocks, and
**cells on the chunk border never merge**. Borders therefore stay identical at every level, and a
crack is impossible by construction, with no skirts and no transition cells. A lone vertex keeps its
exact bits, normal included, or it would show as a shading seam. Measured: 220k triangles become 85k
and 46k, at about 13 µs per chunk. Border locking is what limits the reduction, an accepted price for
having no cracks. Levels switch with distance hysteresis, and collision and light
always use LOD0.

## Rendering

### A separate triangle path

`draw_polygon` allocates three `Vec`s per call and does not cull back faces. That is fine for
hundreds of faces and not for tens of thousands. `mscent-render/src/voxel/` adds:

- **Frame preparation:** each visible chunk's vertices are transformed once, before any tile starts.
  Tile bands share the result read-only, and `TileRenderer` stays deterministic.
- **Triangle fill:** back-face rejection on fixed-point screen coordinates, then fill with a
  **top-left rule on 1/16-pixel snapped vertices**, so shared edges cannot leak or double-draw.
  Triangles crossing the near plane take the existing clipper. Rows outside `fb.row_range()` are skipped.
- **No per-frame allocation:** scratch buffers live with the renderer, and `tests/frame_alloc.rs`
  keeps its limit.

### Visibility

1. **The portal flood is unchanged.** A `Subtract` box is an empty leaf with ordinary portals.
2. **Chunks attach to BSP leaves** at build. `Walk::draw_leaf` draws the leaf's chunks after its
   faces, once per frame, using the same `drawn` idea as faces.
3. **Caution:** `prep::leaf_boxes` builds node bounds from face polygons, and `Walk::visit` culls a
   node with empty bounds. With shell faces gone a volume's leaf would have **no faces and would be
   culled, voxels and all**. Chunk bounds must feed `leaf_boxes`.
4. **Cave flood inside a volume:** breadth-first search from the eye's chunk (or from border chunks
   in view when outside the box), crossing a chunk only if its mask joins the entry and exit faces,
   and never turning back along an axis. It also gives roughly front-to-back order. Measured at 6 µs
   a frame.

### Budget governor

The renderer counts triangles as it submits chunks and pulls the LOD distances in when a frame runs
over `MAX_VOXEL_TRIS`, 25 000 submitted, with hysteresis so levels do not oscillate. Chunks beyond
the fog's total depth are skipped. The adaptive resolution controller remains the last resort. An
overloaded view therefore loses distant detail first, and frame time stays bounded.

### Texturing and shading

- **Projection:** each triangle picks its dominant axis and maps world coordinates through it, with
  the material's scale. UVs depend only on world position, so **textures do not swim when the surface
  is edited**. Triplanar blending would cost three texture fetches a pixel, which the budget cannot pay.
- **Seams and material borders:** a triangle whose vertices disagree on axis or material takes a
  blending shader variant that picks either side per pixel through a 4x4 ordered dither. It is stable
  on screen, cheap, and only those triangles pay. It is the one mechanism for both cases.
- **Light:** colour and dominant direction interpolate perspective-correctly from the vertices. The
  values are attached to world points, so this cannot return the screen-space span crawl that the
  per-pixel lightmap work removed. `examples/crawl.rs` is extended to prove it.
- **Bump and specular:** the tangent axes are the two world axes of the projection, so moving the
  world direction into tangent space is a swizzle, and the existing bump and specular code applies.
  Beyond LOD0, shading is Gouraud plus albedo only.

## Lighting

Faces keep the atlas. Voxels add a **light record per surface cell**, keyed by the global cell
(colour and direction, 9 bytes), about the density of the lightmap at the same size. A vertex in a
border cell reads the record of the chunk that owns the cell, so neighbouring chunks shade identically.

Changes to the baker (`mscent-world/src/lightmap/`):

- **Occluders:** `segment_clear(bsp, a, b)` has two callers (`soft.rs`, `bounce.rs`). Both move to an
  `Occluders` type: the BSP, then a grid walk through the volumes that skips uniform chunks. **Voxels
  shadow brushes and brushes shadow voxels.**
- **Receivers:** a cell is lit like a luxel from a point lifted 1.5 cells along its normal, with the
  same soft shadow sampling. A cell whose point is in solid takes its value from lit neighbours, so
  a stalagmite's root does not get a black rim where it meets the floor.
- **Bounce:** a mixed chunk becomes one patch. Cells gather in 4³ blocks and interpolate, because
  today's bounce is luxels x faces rays and voxels would make that explode.
- **Cache:** `input_key` adds a hash of the resolved field's shape, not its materials, so painting
  never re-bakes. `.msl.lit` gains the records and `store::VERSION` is bumped; the reader's checks
  (counts, finiteness, checksum, ignore and rebake) apply as they do today.

### Light must never flash during a stroke

Today any lighting change drops the 3D view to draft light over the whole level. Doing that on every
dab would strobe. For a voxel-only edit, refinement instead is **local and only moves forward**:

1. **During the stroke:** unchanged cells keep their records, and new cells inherit from the nearest
   lit cells within two cells, darkened by a small occlusion term read from the field. Brush lightmaps
   stay as they are.
2. **150 ms after the stroke** (the existing `QUIET_MS`): hard-shadow rays for changed chunks.
3. **Then the full-quality bake** of changed chunks, whose per-chunk key reuses all the others, and
   the brush lightmap rebake, cancellable as now. `BAKING LIGHT` is shown as now.

## Collision

`sweep_sphere(&Bsp, …)` becomes `sweep_sphere(&Solids, …)`, where `Solids` is the BSP plus the volumes.
It sweeps both and keeps the earlier contact, so `slide_move`, `combat.rs`, `enemy.rs` and radius-0
line of sight need no logic changes. `arena.rs`'s spawn check and the render eye test ask `Solids` too.

A voxel sweep tests chunks the swept box touches, then only triangles in the cells it touches, through
the mesh's per-cell index: the plane pushed out by the radius, then edge cylinders and vertex spheres,
with the primitives in `prim.rs`. This is the same contact model as `cell.rs`, so **a sphere rolling
from a brush floor onto voxel rock meets no invisible lip**. Prototype cost, on a plain scan with no
index: 5.5 µs a sweep at radius 0.5.

The fuzz test in `tests/fuzz.rs` is extended to random levels with volumes, asserting the same two
properties: the sphere never ends within its radius of solid, and never crosses a surface.

The editor's `pick_face` is also blocked by voxels in front of a face, so face picking stays honest.

## Destruction in the game

This extends the requirement list in [plan.md](plan.md); see [open questions](#open-questions).

A weapon hit on a `destructible` material queues a carve dab, at most radius 0.6, on the fixed step.
Keeping the 120 Hz replay guarantee (M4.5) decides the whole structure:

- **Field writes and remeshing are in the simulation, not on a background thread.** Collision must be
  a function of the tick, so a job finishing "whenever" is ruled out.
- **Budgeted and queued:** at most 2 chunk meshes per tick, FIFO. A dab's chunks are built off to the
  side and **swapped together on the tick the last one finishes**, for collision and drawing alike.
  There is never a crack, and render and collision never disagree.
- **Subtract only.** Removing material cannot embed an entity, so no unstuck logic is needed. The ring
  is untouchable, so no hole can reach the void.
- **Light:** new cells inherit, exactly as in a stroke. No ray is traced while playing, so the promise
  in [lightmaps.md](lightmaps.md) holds. The known limit: light does not newly pour through a hole, and
  brush shadows do not change.
- **Resets** on restart, by rebuilding from the level's own data. Damage is not saved.

Worst case is a dab of 8 chunks: 4 ticks, 33 ms from hit to hole, which reads as immediate with a flash.

## Editor

### Painting first, boxes second

The tool is **Voxel (X)**. There is no need to place a box: dabbing is enough.

- **Add** with no volume under the cursor creates an `Add` volume around the dab.
- **Carve on a brush wall** creates a `Subtract` volume around the dab. `base` already matches the
  wall, so **nothing changes visually until the first dab bites**, which is what makes this feel like
  digging into a brush.
- **Dabbing past the box grows it** by whole chunks. An `Add` volume grows on the live path. A
  `Subtract` volume also changes the BSP, so its growth goes through a normal preview, with the
  stroke continuing against the old picture until that arrives.
- **Carving an `Add` volume's underlying brush** converts it to `Subtract`, rewriting deltas so the
  sculpted result is unchanged. The op toggle in the props panel does the same thing.
- Selecting a volume with Select shows its box, which can be moved, reordered and deleted.

### Tools

| Tool | What it does |
|---|---|
| Add / Carve | Sphere, box or capsule dab with a falloff; hold Ctrl while painting to swap |
| Smooth | Blur toward the local average: the CE2 tool that makes everything look natural |
| Flatten | Toward the plane set by the stroke's first hit |
| Roughen | Displace by seeded 3D noise, for weathering |
| Paint | Material only; no shape change, no re-bake |
| Tunnel | Click a path; carves round cones with radius per point |
| Growth | Generators, below |
| Fill | Fills the box from a warped gyroid or noise field, for alien interiors |
| Copy / Stamp | Captures a region and pastes it as a dab |

Radius with the wheel, strength with Ctrl+wheel; hold Shift to lock the stroke to one plane.
Right button remains fly-look, and only the 3D view paints. The 2D views show the box, an outline of
each mixed chunk and the cursor, and they move and resize the box.

### Generators: structure built, not sculpted

All generators output one thing: a bounded list of primitives (sphere, round cone, ellipsoid, each
add or carve, with a material). They are seeded and pure, and they live in the editor. The result goes
through the same dab code, so undo, light and limits need no generator logic.

| Generator | Method |
|---|---|
| Stalactites | Rays up and down from the brushed area find rock; tapered round cones with drip noise |
| Mushrooms | Stem round cone, ellipsoid cap, radial carves for gills, clustered placement |
| Coral | Branching growth with a random bias, then smooth union and Roughen |
| Trees | Space colonization: attractors in a crown, branches grown toward them, thickness from the pipe model, flare into the ground |
| Hive | Hexagonal comb on a surface: carved prisms that leave wax walls, on a curved base |

**Ghost preview:** a generator runs on a background thread and shows the primitives in wireframe in
the 3D view. Wheel re-rolls the seed; click commits one undo step. Radii are clamped to at least one
cell, because thinner than that aliases into broken beads.

### Stroke semantics

Getting these right is most of why a sculpt tool feels solid:

- **Picking uses a snapshot from the start of the stroke.** Otherwise Add moves the surface toward
  the camera and the stroke climbs up its own result. A snapshot is an O(1) `Arc` clone.
- **One stroke does not stack.** Dabs accumulate into a stroke mask by `max`, and the field is
  `op(snapshot, mask)`, so scrubbing an area keeps one even depth. Holding Alt builds up instead.
- **Fast mouse moves leave no gaps:** dabs are spaced a quarter radius along the segment between
  pointer samples, as capsule sweeps.
- **Esc cancels** by restoring the snapshot. **One stroke is one undo step.** On release, islands under
  8 cells are removed, since floating crumbs cost triangles and look like bugs.

### Keeping the UI free

```text
UI thread          pointer -> pick (snapshot) -> dab -> copy-on-write chunks  (doc is now true)
                                                   |
mesher thread      newest dirty set only  <--------+   stale work is replaced, never queued
                   meshes chunks, keeps LOD and light records
                                                   |
render thread      one atomic batch per dab  <-----+   swapped between frames
```

- `Studio::sync_preview` checks **whether only voxel chunks differ**. If so it takes the live path and
  starts no CSG and no draft bake. A brush edit still takes the existing preview path, and meshes for
  chunks whose resolved samples are unchanged come from a cache keyed by their hash, so a brush edit
  far from a volume costs nothing extra.
- Dabs up to radius 2 run on the UI thread: measured about 90 µs to carve and 230 µs to smooth at 0.25
  cells. Radius 4 takes about 0.5 and 1.6 ms, which misses the UI budget, so larger dabs, generators
  and Fill run on a job, are previewed as a ghost and commit whole chunks atomically.
- `Cmd::VoxelBatch` joins the commands that may not be lost, in the worker's `held` queue. Each result
  carries a generation, and an older one than the chunk's current is dropped, which is what keeps undo
  during a running job correct.
- Undo keeps using `Command::EditBrush`, whose clones are now O(1). `History` gains a 256 MiB budget of
  uniquely held chunks, dropping the oldest steps.
- Instant Play uses what the live path holds, once its batches are in. Entering Play stays fast.

## Glitches designed out

| Glitch | Prevented by | Test that reads real output |
|---|---|---|
| Cracks between chunks | Same samples, same vertices | Edge-pair count over a whole volume; pinhole test below |
| Pinholes in triangle fill | Fixed-point top-left rule | Eye inside a closed cave, 1000 random poses at several sizes: **no sky pixel** |
| Hole at the volume and brush seam | Ring plus 1/16 cell overlap bias | Same pinhole test with a tunnel crossing a box wall |
| Z-fighting at the seam | The bias | Second difference of pixels over a slow camera move along the wall |
| LOD cracks | Border cells never merge | Render with LOD0 against all levels: no sky pixel, borders equal |
| Half-updated stroke | Atomic batches with generations | Stress: 1000 dabs and undos while rendering; every frame passes the pinhole test |
| Light flash during a stroke | Local, forward-only refinement | Pixels away from the dab do not change between dabs |
| Black rim at a stalagmite's foot | Dilated records for buried cells | Luminance across the contact line |
| Light crawl | World-fixed vertex values | `examples/crawl.rs` on a voxel shadow edge |
| Texture swimming | World-space UVs | Pixels of an untouched region stay equal across a nearby edit |
| Lost undo state | Immutable chunks | Selftest: stroke, undo, picture equals the original exactly |
| Surface climbing up itself | Stroke-start snapshot | Scripted stroke on a plane: maximum height equals dab height |
| Invisible lips | Same contact model as cells | Slide from brush floor over voxel lip: speed unchanged |
| Tunnelling | Exact sweep | Extended fuzz, both properties |
| Crash on a hostile file | Checked decode with caps | Bit flips and forged RLE, asserted by allocation size |
| Volume disappearing | Chunk bounds feed node bounds | Volume with only shell faces renders |

## Performance

### Measured

Run on this dev machine, release, single thread. Scene: 8x8x4 chunks of 0.25 cells (32x32x16 units) of
warped gyroid, enclosed, every chunk mixed: 220k triangles, which is denser than most real levels.

Field and meshing, from `cargo run --release -p mscent-bench --example voxel_bench`. The machine is
noisy: repeat runs moved these by up to about 25%.

| What | Result |
|---|---|
| Mesh a chunk, with its samples and air mask | about 108 µs mean, 325 µs worst |
| Same, over 8 brushes with a cold `base` | about 59 µs mean, 436 µs worst |
| Cluster a chunk | about 13 µs |
| Mesh memory | 4.1 MiB at 24 bytes a vertex and 6 a triangle, before LOD and light |
| The same cave in a level file | 0.84 MiB, 1.2 ms to read back |
| Carve dab, with copy on write | r=1: about 20 µs, r=2: about 90 µs, r=4: about 500 µs, r=8: about 3 ms |
| Smooth dab, with copy on write | r=1: about 37 µs, r=2: about 230 µs, r=4: about 1.6 ms, r=8: about 12 ms |

Drawing, from a throwaway prototype that M11.4 replaces with a real benchmark, on frames of 640x360
from 40 air positions:

| What | Result |
|---|---|
| `draw_polygon`, frustum only, 62.6k triangles | 15.5 ms, 248 ns each, flat and untextured |
| Prototype path, same triangles | 4 ms, textured with Gouraud light |
| Same, with cave flood | 50.6k triangles, 3.5 ms (worst 9 ms) |
| Same, with flood and LOD at 8 and 16 units | 22.7k triangles, 2.9 ms (worst 75.5k triangles, 5.9 ms) |
| Swept sphere, r=0.5, plain scan | 5.5 µs |
| Today's showcase game frame, for scale | 8.86 ms |

The prototype's LOD row predates the real clustering, which reduces more, so that row should improve.

`cargo run --release -p mscent-render --example voxel_shot` draws volumes beside brushes through
`draw_polygon`, with stand-in light, and counts pixels where the background shows through.

What this says:

- **A submitted triangle costs about 22 ns** beyond fill, from the flood rows with and without LOD (same pixels, 28k fewer triangles, 0.6 ms less). So 25 000
  submitted triangles add about 0.55 ms, 6% of today's frame. That is what makes the plan viable.
- **`draw_polygon` is not an option.** Those 62.6k triangles alone take 15.5 ms of a 16.6 ms frame on
  the dev machine, flat and untextured, because of its per-call allocations and lack of back-face culling.
- **Flood culling is modest on this scene** (19%), because a gyroid is open in every direction. It
  should do better in winding caves, but that is unproven. **LOD does most of the work.**
- **The worst views miss the budget** (75k against 25k) until the governor acts. Cell size 0.5 would
  cut them by about 4x.

What it does not say:

- **The reference machine is unmeasured.** `plan.md` estimates a factor of about 5 for the i3-7100U.
  That would put 25k triangles near 2.8 ms, but it also puts today's 8.86 ms frame near 44 ms, so
  M1.4 and M8.5 remain open for the whole game, not just this.
- **Prototype fill has no bump or specular,** and fills at about 10 ns per pixel against about 38 ns
  for the whole showcase frame. A voxel pixel at High must cost no more than a brush pixel; the prototype
  does not show that. M11.4 measures it.
- **It has no near-plane clipping** and no shader variants.
- **Baking with voxel occluders was not measured.**

### Budgets

Checked on the dev machine by `tools/check_frame_cost.py` once a voxel scene joins it, and on the
reference machine when M8.5 runs.

| Budget | Limit |
|---|---|
| Voxel geometry added to a frame | 15% of the same view's frame time |
| Submitted triangles | 25 000 per frame (`MAX_VOXEL_TRIS`) |
| Per-frame allocations | Unchanged by voxels |
| Worst UI tick during a stroke | 1 ms (today's: 207 µs) |
| Dab to picture | 33 ms |
| Stroke end to draft light of dirty chunks | 250 ms |
| Destruction | 2 chunk meshes per 120 Hz tick |

## Code map

Crate edges do not change: field and meshing live in `mscent-world`, so render and collide see them.

```text
crates/mscent-world/src/voxel/   volume, chunk, context and block (fold, base, ring), prim, dab,
                                 surface (nets), lod, connectivity, wire (section 5), query (grid walk)
crates/mscent-world/src/lightmap/occluders.rs, cells.rs
crates/mscent-collide/src/voxel.rs, solids.rs
crates/mscent-render/src/voxel/  prepare, fill, flood, shader, governor
crates/mscent-game/src/combat.rs (carve), destruct.rs (queue)
apps/editor/src/model/voxel/     tool, stroke, grow, generators/*
apps/editor/src/studio/live.rs   mesher thread, batches
apps/editor/src/ui/voxelpanel.rs
apps/editor/src/selftest/voxel.rs
```


## Alternatives rejected

| Alternative | Why not |
|---|---|
| Compile voxels into the BSP | A leaf per cell breaks `MAX_DEPTH`, CSG time, face merging and the atlas. It destroys what the BSP is good at |
| Build caves from many small brushes | The status quo the request exists to avoid; 348 brushes already take 706 ms and overflow the atlas |
| Final density instead of deltas | Moving a brush leaves stale rock, or forces a "reconform" button that discards sculpting |
| Replay a log of strokes | Load time and edit latency grow with history |
| Marching cubes | About twice the triangles, tables with ambiguous cases, and more slivers. Its sharper planes do not help organic shapes |
| Dual contouring | Sharp features at the cost of a solver and non-manifold cases; brushes already provide sharp features |
| Raymarching the field | Per-pixel stepping cannot fit in a software budget that barely fits a lightmap and bump |
| Transvoxel or skirts for LOD | More code, and either new seams or visible flaps, when border locking gives no cracks at all |
| Atlas lightmaps on voxel triangles | Thousands of tiny rectangles with padding; `atlas_max_height` already overflows at 348 brushes |
| Triplanar texturing | Three fetches per pixel |
| Runtime relighting | Breaks "nothing is traced while playing", and the fallback single-worker path cannot afford it |

## Open questions

1. **Does "destructivity" mean damage while playing?** I took it as yes. The requirements in
   [plan.md](plan.md) limit game elements and need updating to match. If it only meant destructive
   editing, M11.8 can be dropped with nothing else affected.
2. **Should "dynamically constructed" also mean growing while playing?** I took it as procedural
   generation in the editor. Growth at runtime adds material, which can embed entities and needs light
   for cells that never had any, so it was left out.
3. **Reference machine:** the budgets rest on an estimated 5x factor. Real numbers from M1.4 could
   move the default cell size and `MAX_VOXEL_TRIS`.
