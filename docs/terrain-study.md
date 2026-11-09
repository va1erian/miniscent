# Height-field terrain: a study

Status: study only, nothing built. Question: would a heightmap or displacement
system for terrain be simpler and cheaper than the voxel system
([voxels.md](voxels.md))? Measured figures below come from the docs they cite. The
figures marked *estimate* are reasoning, not measurements, and were not prototyped.

## Short answer

- **Cheaper at run time: yes, by a lot, for terrain that really is a height field.**
  A height field answers "is this point in rock?" with one lookup instead of a
  nearest-surface search. It can skip most shadow rays with a min/max tree. It meshes
  a regular grid and stores one number per column, not a 3D grid of chunks.
- **Simpler for the codebase: no.** Voxels cannot go: caves, overhangs, growths,
  sculpting, destruction and the arena all need 3D. A height field would be a second
  surface type beside voxels, in the format, collision, the bake, both renderers and
  the editor. That is roughly 3 000 to 4 000 new lines (*estimate*) against the voxel
  system's 12 300.
- **Today it would buy little.** The only height-field terrain in the repository is
  the three `mountains` ranges of the vista demo. They sit in a layer, where nothing
  collides, and after M13.9 they already cost 272 chunks and 380 KB at 4-unit cells.
  One 256-wide range at scale 16 bakes in 197 ms. What they still cost on screen is raster time, and a height field changes
  that little.
- **Recommendation:** do not build it now. Build it when levels get large walkable
  outdoor ground. Then build it as a voxel *volume kind* that shares the voxel draw
  path, not as a separate renderer feature. "Next steps" lists the measurements that
  would settle it first.

## What exists

| Fact | Where |
|---|---|
| `mountains` is already a height field: `Heights` holds one height per column (a ridged multifractal), and `Heights::field` turns it into a pseudo-distance that `fill` writes into a 3D `Add` volume. Nothing about the result needs 3D | `crates/mscent-world/src/voxel/mountains.rs` |
| A volume is i8 deltas over the brush CSG, 1/32 cell per step, one material per sample, in 16³ chunks, at most 16 chunks a side | `voxel/mod.rs`, [voxels.md](voxels.md) |
| Meshing is surface nets per chunk, LOD by 2³ and 4³ clustering, and border cells never merge, so triangle caps are soft | `voxel/surface.rs`, `voxel/lod.rs` |
| Light is one 9-byte record per surface cell, lit from 1.5 cells off the surface, Gouraud between vertices. Brush faces are lit per luxel | `lightmap/cells.rs` |
| Levels that need 3D: `voxel-cave`, `voxel-hive`, `voxel-mushrooms`, `voxel-tree`, `voxel-coral`, and the game's arena. Pure height field: the three ranges of `facing.txt` | `assets/levels`, `crates/mscent-game/src/arena.rs` |

## Where the voxel system pays, and what a height field would pay

| Cost | Voxels (measured) | Height field |
|---|---|---|
| `is_solid` | about 17 µs, against 0.17 µs for the BSP alone ([voxels.md](voxels.md)) | A bilinear lookup and a compare, about the BSP's cost (*estimate*) |
| Sphere sweep, 4 units | about 46 µs; `slide_move` about 33 µs | The triangles of the few columns under the swept box, a few µs (*estimate*) |
| Particles | A voxel test is weighted 16 tests, so a crowd thins to about 32 voxel tests a tick (`fx/collide.rs`) | No thinning needed (*estimate*) |
| Shadow ray | about 2 µs against voxels and 0.2 against the BSP; a bake with voxel shadows takes 410 to 450 ms against 70 to 81 | A 2D walk over a min/max height tree that stops where the ray is above the tallest point of a block. Long rays over open ground leave the tree almost at once (*estimate*) |
| Meshing | about 108 µs a chunk; 70 ms for 256 chunks single-threaded | A regular grid: no search, nearly free (*estimate*) |
| LOD | Clustering; caps are soft because borders never merge | Geomipmapping or CDLOD with skirts: hard caps, and far flat ground collapses to a few triangles |
| Storage | The demo's ranges: 272 chunks, 380 KB at 4-unit cells (4.8 MB at 1-unit cells) | One u16 a column: a 256 by 256 range is 128 KB raw, less after RLE |
| Raster | Voxel pixels 51 to 57 ns against 30 to 50 for a brush; the gate pose 13 168 triangles, 1.4 times a wall at High | Same pixels if it reuses the voxel span path; fewer triangles far away. Pixels dominate, so the gain is small |

## Two ways to build it

**A. A height-field volume kind (recommended if built).** It would be a new entry
beside `Add` and `Subtract` volumes: a column grid, a cell size and per-column
materials. It meshes to the same `ChunkMesh` the voxel draw path takes, so both
renderers, the material blends and the triangle budget keep working unchanged.
Collision and shadow rays get a fast path. Sculpting with the height tools (raise,
smooth, flatten, paint) edits columns. A carve tool turns the region into an ordinary
voxel volume. `mountains` would write columns instead of a 3D field, keeping its seed
and output.

**B. Displaced brush faces, as Source does.** A brush face gets a 2^n grid of offsets
along its normal. Its light goes through the lightmap atlas as a face does, so it
would meet a brush floor with exactly the same light. That is the seam this branch
works on for vistas, and voxel ground, lit per vertex, cannot match it. But the BSP,
portals and the leaf flood know nothing of the displacement, so collision, shots,
particles and visibility need a second path anyway. It also stays one face's size:
not a 1000-unit range.

## Next steps, if outdoor play ground becomes a goal

1. Prototype `is_solid` and a sphere sweep on a 256 by 256 height grid in
   `mscent-collide`, beside the voxel figures above.
2. Prototype a min/max-tree shadow ray in `lightmap/voxel_rays.rs` terms, and bake
   `facing.txt`'s ranges both ways.
3. Decide from those two measurements. Raster cost should not drive the decision;
   it barely moves.
