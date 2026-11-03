# Many dynamic lights on the hardware renderer

Status: draft plan, not started. Work items are M17 in [work-items.md](work-items.md) once
accepted. Builds on [hardware-renderer.md](hardware-renderer.md) and sits beside
[hardware-effects.md](hardware-effects.md).

## Goal

A light for every shot in flight, hundreds at once, with little cost to the frame on the
GPU. The software renderer keeps its 4 lights unless more is cheap there. Parity between
the renderers is dropped by the owner's decision of 2026-11-03: gameplay must be
identical, looks need not be. Lights are cosmetic, so they never touch the simulation or
replays.

## What limits lights today

- The cap of 4 (`MAX_DYN_LIGHTS`) is in `Frame` in `common.wgsl`, an assert in
  `dyn_lights.rs`, the 4-bit per-face mask, and the game's `fx/lights.rs` slots.
- A light lights only faces its portal flood reached (`DynLight::place`, once per light,
  not per frame). With no shadows, that flood is also what stops a light shining through a
  wall. Any design past 4 lights has to keep it.
- `dynamic_light()` uses the face's geometric normal only, not the bumped one.
- Gameplay is hitscan. A bolt (`fx/combat.rs`, 32 at most) is a cosmetic streak and glow.
- `examples/light_bench.rs` (software, 640x360 High, one thread) reads 5.64, 5.96, 6.61 and
  6.92 ms for 0, 1, 2 and 4 lights, about 23 ns a span per light and linear. The GPU
  cost per light has not been measured.
- A light's reach is a portal flood by distance only (`scene/reach.rs`). It tests no
  occlusion inside a room: a pillar does not stop a light.

## Options

| Option | Verdict |
|---|---|
| Raise the cap and loop every light per pixel | Cost is linear in lights over the whole frame. No |
| **Tiled forward: a light list per screen tile, read by the existing shaders** | **Chosen.** Cost follows lights per tile. Keeps MSAA, cutouts, vistas and voxel blends |
| Full deferred (G-buffer) | A face's colour needs albedo, normal, gloss row, baked light and flags per pixel, which is heavy at 4K. the High finish is 4x multisampled. Soft cutouts, vista layers and three-material voxel triangles do not fit one G-buffer. No |
| Light pre-pass (depth, then light volumes into an accumulation buffer) | Still needs a leaf per pixel for the wall rule and lights multisampled edges at one sample. Its only gain, lighting no overdrawn pixel, comes from a depth pre-pass in the forward design |

## Design

1. **Light list.** One storage buffer of up to `GPU_LIGHTS` (512): position, radius,
   colour. `Frame` keeps the strongest 4 for software and for actors, whose light is
   taken on the CPU.
2. **Cluster binning on the CPU.** Clusters, not flat tiles: 16x16 pixels by 16 to 24
   logarithmic depth slices, because in a 6DoF tunnel a flat tile collects the lights of
   every depth along the view. Each light is added to the clusters its sphere touches, at
   most 32 a cluster, dropping the dimmest. A few hundred lights is trivial work, and the
   project uses no compute shader today. The lists go to two storage buffers: a count and
   offset a cluster, and the indices. The cluster count at 4K is checked in M17.1.
3. **Shader.** The loop in `dynamic_light()` (`face.wgsl`) and `with_dynamic` (`voxel.wgsl`)
   reads the fragment's cluster instead of the face mask. The cluster comes from
   `@builtin(position)` and the view depth.
4. **The wall rule, by room.** Suggested by the engine-review session, which found the
   leaf version below ambiguous. At compile time, empty leaves merge into rooms and a
   room to room visibility table is stored (a bit table, a row a room). A face and a voxel
   chunk carry a room id. A light lights a pixel only if the pixel's room bit is set in
   the light's row, one lookup, so a moving light needs no re-flood: it only needs the
   room it is in. A face is one-sided, so it takes the room it faces. A voxel chunk that
   spans rooms takes the union of its rooms' bits, which can leak a little light across a
   thin wall. Whether leaves merge cleanly into rooms in this engine's BSP, and what that
   costs the compiler and the level file, is checked in M17.2 before anything is built on
   it.
   - *Rejected: a bitset of reached leaves a light, with a face to leaf table.* A face
     borders several leaves and a chunk spans several, so the table is ambiguous, and a
     moving light's reach had to be approximated from a cached leaf. If the rooms do not
     merge cleanly, this is the fallback, with those costs.
   - `FaceMasks::write` today loops over every face of the level each frame with a lit
     light, not only the visible ones. Both versions remove that loop.
5. **Software.** The software renderer is not changed and does not read the cluster lists:
   it keeps its 4 lights and its `FacePoints` slots, so LazyOS pays nothing. It takes the
   strongest 4 of the full list. If a cheaper way to show more lights appears there, it is
   welcome but not required.
6. **Overdraw.** No depth pre-pass at first. If the light loop shows overdraw in the
   measurement, a depth-only pass of the faces comes before the lit pass.

## Later: real occlusion for every light (optional, GPU only)

A coarse 3D occupancy or distance grid of the solid world (brushes and voxels together),
ray-marched a few steps per light per pixel, would occlude every light, not only the two
with shadow maps ([hardware-effects.md](hardware-effects.md)). The same grid could serve
AI line of sight. It is a feature of its own, with a grid to build and keep in step with
sculpts, so it is a candidate for after M17.4 and is not planned here. Room visibility
stays as the cheap first test either way.

## Pairing with real projectiles

The user's intent is for this to pair with a move from cosmetic tracers to projectiles
that exist in the world, each carrying a light. That is gameplay work, separate from the
renderer and the owner's call:

- A projectile entity with a swept-sphere test each sim step, so a fast one cannot tunnel
  through a thin wall or enemy, and motion that replays exactly.
- Hit and damage move from `hitscan` to the projectile's arrival. `weapon.rs`, `combat.rs`,
  the enemy code, the scripting docs and the replay tests change with it.
- The light is cosmetic: the sim does not read lights (`fx/lights.rs`), so replay is
  unaffected. It follows the projectile's position interpolated between sim steps.
- Bolts become the projectile's visible body; `MAX_BOLTS` and the tracer particles go.
- Count: player and enemy shots plus impacts, a few hundred at most, hence 512.

The lights work without projectiles: impacts, muzzle flashes and cosmetic bolts can
use the larger list first, so the two halves can land apart.

## Work items (proposed, M17)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M17.1 | Measure: a throwaway branch raising the cap, timing `gpu_shot` at 4K with 4, 16, 64 and 256 lights, and `scene.light_reach` per light | | S | Numbers in `docs/perf/`, and this plan's costs updated or the plan dropped |
| M17.2 | Wall rule by room: merge empty leaves into rooms, a room visibility table, room ids on faces and chunks, at 4 lights still. Also stops `FaceMasks::write` visiting every face | | M | Existing renderer tests pass; a test where a light beside a wall leaves the far room dark, on the old mask and the room table alike; the table's size and compile time recorded. If rooms do not merge cleanly, falls back to per-leaf bitsets |
| M17.3 | Cluster lists: `FrameInput` carries the full list, CPU binning, buffers, both shaders read clusters | M17.1, M17.2 | L | 256 lights draw; a frame with at most 4 lights equals the GPU's own frame from before, byte for byte; each cluster's list checked against a brute-force count in a test |
| M17.4 | Moving lights: leaf-crossing re-flood, cached reach, game slots lifted from 4 to `GPU_LIGHTS`, software takes the strongest 4 | M17.3 | M | `light_bench` and a flight of 256 lights show frame cost within budget; replay unchanged |
| M17.5 | Real projectiles: entity, swept test, hit and damage, replay, scripts, bolts removed | | L | Hit-for-hit equal to hitscan at close range, tunnelling tests on thin walls, replay bit-exact. Owner's decision |
| M17.6 | Speed and limits: tile caps, dimmest-dropped rule, record in `docs/perf/` | M17.4 | S | 4K cost with 256 lights recorded against budget |

M17.2 and M17.1 are independent and can run side by side.

## Tests

- **Few lights unchanged.** Frames with at most 4 lights equal the GPU's own earlier bytes.
  This is a regression check, not a comparison with software.
- **Cluster lists.** A test builds the lists for a scene of random lights and compares each
  cluster with a brute-force test of every light against the cluster's box.
- **Truth for many lights.** A grey room with 64 lights on a grid: a pixel under each is the
  sum of its neighbours' hand-computed falloffs, within the shading's rounding.
- **Walls.** A light in the next room lights nothing here, with 1 light and with 256.
- **Cap and drop.** More lights than a tile holds drop the dimmest, and the picture still
  draws; a hostile count or a NaN position is refused, not a panic.
- **Seeded.** Break each piece once (cluster off by one, room test off, wrong room for a moving
  light) and record that its test fails.

## Risks

- **Wall rule by room.** The room table is coarser than the flood: it lets a light into a
  whole room it can see any part of, where the flood stopped at its radius. The radius and
  the falloff already cut that down, and a doorway test decides whether the leak shows.
  Merging leaves into rooms is the unproven part (M17.2).
- **Cluster overflow.** A hot spot of many overlapping flashes drops the dimmest in a cluster,
  which reads as a flicker if the choice is unstable. Order by strength, with a tie
  broken by index.
- **No software check.** More than 4 lights has no software twin and parity is dropped, so
  only the regression check and the truth tests keep the shared shading honest.
- **Projectiles are a design change.** M17.5 touches gameplay, replay and scripts. It is not
  needed for the lights, and can wait or be dropped.
