# Many dynamic lights on the hardware renderer

Status: draft plan, not started. Work items are M17 in [work-items.md](work-items.md) once
accepted. Builds on [hardware-renderer.md](hardware-renderer.md) and sits beside
[hardware-effects.md](hardware-effects.md).

## Goal

A light for every shot in flight, hundreds at once, with little cost to the frame on the
GPU. The software renderer keeps its 4 lights. The extra lights are a GPU-only look under
`Finish::Fine`, by the same exception as the shadows and parallax plan.

## What limits lights today

- The cap of 4 (`MAX_DYN_LIGHTS`) is in `Frame` in `common.wgsl`, an assert in
  `dyn_lights.rs`, the 4-bit per-face mask, and the game's `fx/lights.rs` slots.
- A light lights only faces its portal flood reached (`DynLight::place`, once per light,
  not per frame). With no shadows, that flood is also what stops a light shining through a
  wall. Any design past 4 lights has to keep it.
- `dynamic_light()` uses the face's geometric normal only, not the bumped one.
- Gameplay is hitscan. A bolt (`fx/combat.rs`, 32 at most) is a cosmetic streak and glow.
- `examples/light_bench.rs` (software) reads about 23 ns a span per light, linear. The
  GPU cost per light has not been measured.

## Options

| Option | Verdict |
|---|---|
| Raise the cap and loop every light per pixel | Cost is linear in lights over the whole frame. No |
| **Tiled forward: a light list per screen tile, read by the existing shaders** | **Chosen.** Cost follows lights per tile. Keeps MSAA, cutouts, vistas and voxel blends |
| Full deferred (G-buffer) | A face's colour needs albedo, normal, gloss row, baked light and flags per pixel, which is heavy at 4K. `Finish::Fine` is 4x multisampled. Soft cutouts, vista layers and three-material voxel triangles do not fit one G-buffer. No |
| Light pre-pass (depth, then light volumes into an accumulation buffer) | Still needs a leaf per pixel for the wall rule and lights multisampled edges at one sample. Its only gain, lighting no overdrawn pixel, comes from a depth pre-pass in the forward design |

## Design

1. **Light list.** One storage buffer of up to `GPU_LIGHTS` (512): position, radius,
   colour. `Frame` keeps the strongest 4 for Reference and for actors, whose light is
   taken on the CPU.
2. **Tile binning on the CPU.** Each light is projected to a screen rectangle and added to
   every 16x16 tile it touches, with at most 32 lights a tile, dropping the dimmest. A
   few hundred lights is trivial work, and the project uses no compute shader today. The
   lists go to two storage buffers: a count and offset a tile, and the indices.
3. **Shader.** The loop in `dynamic_light()` (`face.wgsl`) and `with_dynamic` (`voxel.wgsl`)
   reads the fragment's tile instead of the face mask. A tile is found from
   `@builtin(position)`.
4. **The wall rule, per leaf.** The per-face mask becomes, per light, a bitset of the BSP
   leaves it reached, with a static face to leaf table (a voxel chunk takes its leaf).
   The shader tests one bit a light. 512 lights over 1,000 leaves is 64 KB. The `n.l`
   clamp and the radius already do what "faces it" did. A moving light re-floods only
   when it crosses into another leaf, using the reach cached for that leaf and a small
   radius margin, so a flying shot costs no flood each frame.
5. **Order of sums.** Lights add in index order, so up to 4 lights the Reference sum is the
   same as now and the parity rows are unchanged.
6. **Overdraw.** No depth pre-pass at first. If the light loop shows overdraw in the
   measurement, a depth-only pass of the faces comes before the lit pass.

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
| M17.2 | Wall rule per leaf: face to leaf table, per-light leaf bitsets, at 4 lights still | | M | The parity suite passes untouched; a test where a light beside a wall leaves the far room dark, on the old mask and the new bitset alike |
| M17.3 | Tiled lists: `FrameInput` carries the full list, CPU binning, buffers, both shaders read tiles | M17.1, M17.2 | L | 256 lights draw; Reference byte-equal to before; each tile's list checked against a brute-force count in a test |
| M17.4 | Moving lights: leaf-crossing re-flood, cached reach, game slots lifted from 4 to `GPU_LIGHTS`, software takes the strongest 4 | M17.3 | M | `light_bench` and a flight of 256 lights show frame cost within budget; replay unchanged |
| M17.5 | Real projectiles: entity, swept test, hit and damage, replay, scripts, bolts removed | | L | Hit-for-hit equal to hitscan at close range, tunnelling tests on thin walls, replay bit-exact. Owner's decision |
| M17.6 | Speed and limits: tile caps, dimmest-dropped rule, record in `docs/perf/` | M17.4 | S | 4K cost with 256 lights recorded against budget |

M17.2 and M17.1 are independent and can run side by side.

## Tests

- **Reference unchanged.** Frames with at most 4 lights equal their former bytes at
  `Finish::Reference`.
- **Tile lists.** A test builds the lists for a scene of random lights and compares each
  tile with a brute-force test of every light against the tile's rectangle.
- **Truth for many lights.** A grey room with 64 lights on a grid: a pixel under each is the
  sum of its neighbours' hand-computed falloffs, within the shading's rounding.
- **Walls.** A light in the next room lights nothing here, with 1 light and with 256.
- **Cap and drop.** More lights than a tile holds drop the dimmest, and the picture still
  draws; a hostile count or a NaN position is refused, not a panic.
- **Seeded.** Break each piece once (tile off by one, leaf test off, no re-flood on a
  move) and record that its test fails.

## Risks

- **Wall rule at scale.** A moving light's reach from a leaf cache is an approximation of a
  flood from its exact position. Light could reach a room by a hair at a doorway. The
  margin and a doorway test decide it.
- **Tile overflow.** A hot spot of many overlapping flashes drops the dimmest in a tile,
  which reads as a flicker if the choice is unstable. Order by strength, with a tie
  broken by index.
- **Parity blind spot.** Same as the shadows plan: more than 4 lights has no software
  twin, so only the Reference-unchanged test keeps the shared shading strict.
- **Projectiles are a design change.** M17.5 touches gameplay, replay and scripts. It is not
  needed for the lights, and can wait or be dropped.
