# Hardware renderer: projected shadows and parallax mapping

Status: accepted plan. M16.1 (the switches) and M16.2 (parallax) are built, with what was
measured in [status.md](status.md), M16. Shadows are built as a first slice (M16.3, and the
casters of M16.4 that are enemy models, actors and movers): see "Shadows as built" below
for what is there and what is left. The View menu toggles (M16.5) are not. Work items are
M16 in [work-items.md](work-items.md) once accepted. Builds on
[hardware-renderer.md](hardware-renderer.md).

Where parallax as built differs from the plan below: a material's depth defaults to 0, so
only a `materials.txt` line that gives one (brick 4, rock 5) draws relief; a map is lifted
so that its highest texel is the face's plane, which makes a flat map no relief; the offset
is not clamped to the face's rectangle; the effects are `GpuEffects`, set on a
`GpuRenderer`, and follow the frame's quality when none is set.

## Goal

Two looks that only the GPU draws. Parity between the renderers is dropped by the
owner's decision of 2026-11-03: the game must play identically on either, but need not
look identical.

- **Projected shadows** from the frame's dynamic lights, cast by what moves and what
  stands in the light's way.
- **Parallax mapping** on brush faces whose material has a height map: relief that
  shifts with the eye, where bump only changes the light.

The software renderer gets neither. It stays the fallback and LazyOS's renderer, and
its pictures do not change; it only has to look good enough for what a LazyOS box can do.

## What is there today

- **Static shadows are already baked** into the lightmap, soft, with no crawl
  ([lightmaps.md](lightmaps.md), status.md "Shadow stability"). Projected shadows must
  not add to them.
- **A dynamic light lights without occluding.** `dynamic_light()` in `face.wgsl` is
  `n.l * falloff^2`. `reaches_face` and `reaches_leaf` only choose which faces and
  leaves a light may touch, through portals. So a lamp behind a pillar lights the
  floor behind it, and an actor casts nothing. This is the gap the shadows fill.
- Up to `MAX_DYN_LIGHTS` (4) lights per frame, in `Frame`. Actors take theirs on the
  CPU, voxels a chunk, faces a bit mask a face.
- `Texture` holds an optional 8-bit height map (`name_h.png`, `with_heights`). Nothing
  on the GPU reads it, and whether the Texture Studio bake writes it is the first
  thing M16.2 checks.
- Bump (normal map plus baked direction) is per pixel on the GPU, faded by
  `BUMP_FULL` and `BUMP_GONE`.

## The parity policy

Dropped outright by the owner (2026-11-03). The gate is gameplay: the simulation,
collision and replays are bit-exact whichever renderer runs. Lights, shadows and relief
are cosmetic and never reach them. What this plan does about it:

1. **No parity rows, no limit moves.** Neither effect gets a row in
   `crates/mscent-gpu/tests/parity.rs`. Rows that exist stay while they pass, and are
   not edited to make room for these looks. A limit in `mscent_math::epsilon` is not
   loosened to pass anything.
2. **The effects follow quality, not the finish.** They draw at High, and a switch in the
   View menu and for tests turns each off. They need not hide behind `Finish::Fine`.
3. **Truth tests instead.** Each effect is held to a truth that needs no software frame
   (see Tests), and to its own earlier output with it switched off.
4. **Words change.** `AGENTS.md`, the "Not in scope" bullet of
   [hardware-renderer.md](hardware-renderer.md) and the two `.claude/agents/`
   definitions are rewritten by the owner's separate change that drops parity; this plan
   adds nothing to them. Until it merges, those texts still describe the old rule.

## Projected shadows

### Approach: cube shadow maps for the lights that matter

| Option | Verdict |
|---|---|
| **Cube shadow map per light, from the casters** | **Chosen.** Handles every caster and receiver the same way, cost bounded by light count and resolution |
| Stencil shadow volumes | Needs closed, silhouette-extruded geometry; voxels and mesh actors make that costly, and fill rate suffers at 4K |
| Planar projected blob under actors | Cheap, but only floors, and not a real shadow on walls or other actors |
| Analytic sphere or capsule shadows for actors | Kept as a fallback for a weak GPU (a later option), no map needed, no crawl |

**Only the dynamic term is shadowed.** `k += dynamic_light(...)` is multiplied by the
light's visibility. The baked term stays as it is, so a wall already shadowed by the
bake is not shadowed twice, and a lamp shadowed by a pillar now is.

### Passes

A new `shadows.rs` and `shadow.wgsl`, run before the scene pass, in the frame list
after the visible set is known:

```
visible = assets.visible(view)
shadows::draw(encoder, lights, casters)   // NEW, High only
sky, movers, voxels, faces, actors, particles ... as now
```

- **Which lights.** At most `SHADOW_LIGHTS` (start at 2) of the lit lights, chosen by
  brightness over distance to the eye, with hysteresis so two lights near a tie do not
  swap their shadows frame to frame. The rest light without shadows, as today.
- **Map.** A cube per shadow light, 512 or 1024 a side, `R32Float` holding distance to
  the light over its radius, with a depth buffer. Written as distance, not projected
  depth, so the shader needs no per-face matrix and no reversed Z.
  Six passes per light (wgpu has no multiview here).
- **Casters, from lists the renderer already has.**
  - Faces and movers: the light's own reach (`reaches_face`), which already includes
    only what the light can see through portals, so no new culling.
  - Voxel chunks: those whose bounds meet the light's sphere, at the coarse level of
    detail. Actors and models: their existing vertex buffers.
  - Cutout texels cast as solid in v1 (decision below).
- **Receivers.** Faces and voxels in `face.wgsl` and `voxel.wgsl`. Actors and models take
  their dynamic light on the CPU today, so they receive no shadows in v1 and cast them.
  Moving actor light onto the GPU is separate work.
- **Sampling.** Direction from the light to the point picks the cube face. Slope-scaled
  bias plus a normal offset, and 3x3 PCF with the kernel widened by distance from the
  occluder to soften the edge. The kernel is the answer to the crawl the owner
  complained about ("shadows that crawl and jag the way shadow maps do"): without it
  this feature is that complaint again.
- **Cost guard.** Skipped when no light is lit, and per light when its sphere meets
  nothing movable and the static scene is unchanged is *not* skipped: static casters are
  in the map, so a still map could be cached between frames while the light and its
  casters do not move. That cache is the first optimisation if the measurement needs it.

### Decisions (taken 2026-11-03, the owner accepting the recommendations)

1. Static geometry casts into the dynamic light's map, as otherwise a lamp lights
   through walls and pillars. It costs the most.
2. Cutout texels (leaves, grilles) cast solid in v1.
3. Shadows at High only.

### Shadows as built (first slice)

`crates/mscent-gpu/src/shadows.rs`, `shadow_lights.rs`, `mover_casters.rs`,
`shaders/shadow.wgsl` and `shaders/shadow_sample.wgsl`, with `face.wgsl` calling the
lookup. Tests are `tests/shadows.rs` and `tests/shadow_off.rs`; numbers are in
[status.md](status.md), M16.3, and [perf/hardware.md](perf/hardware.md).

What is built, as planned unless said:

- **Maps.** `SHADOW_LIGHTS` (2, in Rust, given to the shader as an override) cube maps of 768
  a side in one `R32Float` array, holding distance over the light's reach, with a shared
  depth buffer, six passes a light before the scene pass. Made with the renderer (28 MB; 512
  looked blocky where a light is close to the floor and a surface is large on screen, and
  1024 cost no less to look at but 0.3 ms more); a
  GPU too small to hold them has maps of a texel and draws no shadows. The caster pipelines
  are compiled once per device.
- **Which lights.** The brightest over their distance to the eye among the frame's lit
  lights; a light keeps its map until another is 1.25 times stronger, so a tie does not swap
  frame to frame (`shadow_lights.rs`, unit tests). A light that is dark, or whose position or
  radius is not finite, is never a candidate.
- **Casters.** Enemy models and the player's ship (their part's storage as the voxel
  pipeline reads it, one matrix a part, parts culled by a sphere against the light's sphere;
  listed for the maps apart from what the camera draws, so a model out of view, or one that
  is only a caster, still casts: `ModelInstance::cast_only`, `ModelList::casters_unprojected`,
  a second `ModelPass` that writes no frames or materials), actors (their triangle
  list, all of it, in each light's passes) and movers (every face, not just those that face
  the camera, at their poses, culled by a sphere). Both sides of a triangle cast.
- **Receivers.** Lit brush faces in `face.wgsl`: each light's term in `dynamic_light()` is
  multiplied by its visibility, and the baked term is not. A mover's own faces are not
  looked up, as their dynamic light is already taken in their authored frame.
- **Sampling.** `shadow_visibility`: the receiver moved off its plane along its normal; its
  depth taken on its own plane at each tap, not at one point; a small slope-scaled bias left
  over; eight blocker taps find the nearest occluder, the penumbra width comes from the light's
  size (0.5 units) over that distance and clamps to 2 to 16 texels of radius; 24 taps of a
  fixed Vogel disc. The taps are not turned per pixel: a turned pattern left grain that
  measured ten times the stair-step of the fixed one (0.28 against 0.03 px).
- **Off.** With shadows off, or no light with a map, the frame is the frame before this
  work, byte for byte (`tests/shadow_off.rs` holds hashes taken at b3c0e6d).

What differs from the plan above, and what is left:

1. **Static faces and voxel chunks are not casters yet** (decision 1, the costliest). A
   pillar or a wall does not shadow a dynamic light, so a lamp behind a pillar still lights
   what is behind it, as today, and a model's shadow does not reach a surface the model's
   own light cannot. Adding them is a list in `shadows::Casters` and a draw in
   `ShadowPass::draw_casters`: faces take the movers' pipeline with pose 0 and an index
   list culled by `reaches_face`; chunks take the voxel vertex buffer. The follow-up also
   owns the light-through-a-wall-corner test, which this slice can only run on two boxes that
   touch (`light_does_not_pass_between_two_boxes_that_touch`).
2. **Voxel faces and enemy models do not receive.** Their dynamic light is a vertex
   property computed on the CPU (models, actors) or per chunk (voxels), so neither darkens in
   a shadow. A model in another model's shadow is lit as before.
3. ~~Models cast only when the camera lists them.~~ Fixed with the player's ship: models are
   listed for the maps whatever the camera sees (test `a_model_the_camera_does_not_see_still_casts`).
   They are not yet culled per light before listing, only per part when drawn, and actors are
   not culled per light.
4. **Actors cast the triangles that face the camera**, as `actor_triangles` drops the others,
   so a shadow from the camera's own side of an actor is a little short at its edge.
5. **Cutouts cast solid**, as decided. Models' voxel levels of detail follow the camera, not
   the light.
6. **A still map is redrawn every frame.** The cache is the first optimisation if the
   measured cost (about 1.1 to 1.3 ms in a room with two lights and six enemies, 4K) needs
   it; a cheaper receiver (fewer taps where the kernel is small) is the other.
7. The View menu toggle is M16.5. The game has `--chase` for the ship (below).

### The player's ship

`assets/ship/player.msb` (766 bytes, 144 triangles, the stock skins plating, metal and
glass): a hull, swept wings, a fin and an engine pod about a unit and a quarter long, authored
in the player's own frame (nose -Z, up +Y) so that the player's orientation turns it as it is.
`apps/game/src/ship.rs` makes the instance each frame from the interpolated position and the
view's orientation, bank included; nothing reads it back, so collision, aim and replays do not
see it. It exists only in a build with the `hardware` feature, which keeps the software build
its size (and software draws nothing of it anyway). In first person it is `cast_only`: never
drawn, always a caster. `--chase` (`GameSession::set_chase`) puts the camera 3 behind and 0.8
above it and draws it, to look at. Not done: shots and enemy fire do not yet make lights with
a shadow-casting priority of their own; the ship shadows whichever lights the frame has.

## Parallax mapping

### Approach: parallax occlusion mapping in the face fragment shader

- **Data.** Heights upload as an `R8Unorm` texture with a mip chain (built by the
  existing `mips` code, not the GPU's), one per material that has them, in the
  material's bind group. A face flag `HAS_HEIGHT` says so, like `HAS_NORMALS`. A
  material without heights draws exactly as now.
- **Depth of the relief.** A per-material depth in texel-space units, from an optional
  column in `materials.txt` (ids and old lines unchanged), with a default constant for
  materials that give none. The constant is a look setting, not a tolerance, so it
  sits with `BUMP_FULL`, not in `epsilon`.
- **Method.** In `face.wgsl`, before the albedo is fetched: take the eye vector in the face's
  tangent frame (already built in `with_highlight`), march the height field along it with
  a step count from view angle, refine the hit with one secant step, and offset
  `corner.texels` by the result. Albedo, normal, height and the cutout test all use the
  offset coordinate.
- **Light stays put.** The luxel position and the baked light use the unshifted point.
  Light is low frequency, and shifting it would turn the lightmap seams visible.
- **Fades like bump.** The relief eases out between `PARALLAX_FULL` and `PARALLAX_GONE`
  texel rates, so distant walls cost nothing and do not shimmer. The step count falls
  with it.
- **Derivatives.** The offset coordinate is computed in uniform control flow and its
  gradient taken from the *unshifted* texels, so the sampler's level selection and the
  anisotropic filter do not see the march's discontinuities.
- **Left out of v1.**
  - Silhouettes at edges: the offset can run off the face. Clamp to the face's
    texel rectangle and fade, and do not write `frag_depth`, which would turn off early
    depth testing for every face.
  - Self-shadowing of the relief against the baked direction and the dynamic lights.
    This is the natural second step and the two features then interact, so it waits.
  - Voxel surfaces: their texels come triplanar from a storage buffer with no height
    plane. Skipped, and voxels keep bump only.
  - Cutout materials (`MASK` 1 and 2) and panned faces (`PANS`): not marched.

### Decisions (taken 2026-11-03)

1. Brick and rock get heights first. The Texture Studio recipe may need an
   `out-height` if it is not written already.

## Work items (proposed, M16)

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M16.1 | Seam: a per-effect switch (shadows, parallax) in the GPU renderer's settings and the View menu, on at High; no shader change | the owner's parity change merged | S | A frame with both off equals the frame before the switch, byte for byte; the switch is read by the editor and the game |
| M16.2 | Parallax: check the height bake, upload heights, `HAS_HEIGHT`, the march, the fade, the per-material depth | M16.1 | M | Tests below; `facing.msl` and `showcase` draw; a hostile height map (wrong size, no mips) is refused |
| M16.3 | Shadows, faces: cube pass, light choice with hysteresis, faces as casters and receivers, PCF | M16.1 | L | Tests below; a lamp behind a pillar leaves its far side dark |
| M16.4 | Shadows, the rest: movers, voxels, actors and models as casters, voxels as receivers | M16.3 | M | A moving actor's shadow crosses a floor and a wall; voxels shadow and are shadowed |
| M16.5 | Editor and game: View menu toggles for each effect, `--renderer software` logs that they are off, perf pass | M16.2, M16.4 | M | Costs recorded in `docs/perf/`; a person has looked at it in a real window |

M16.2 and M16.3 are independent and can run side by side.

## Tests

As everywhere: real pixels. Neither effect has a software frame to compare with, so each
is held to a truth.

- **Off is a no-op.** A frame with both effects switched off equals the frame from before
  this work, byte for byte. This is a regression check on the GPU's own output, not a
  comparison with software.
- **Shadow, geometry.** A flat grey room, a point light and a box of known size. The pixel
  on the floor where the box's shadow falls by hand computation is under 10% of the lit
  value; a pixel beside it is lit; fails with the caster dropped from the map.
- **Shadow, no double dark.** A wall already in the bake's shadow is no darker with the
  map on, within 1 level (the bake and the map must not multiply).
- **Shadow, stability.** The `crawl` probe's contour and noise measures on a shadow edge
  as the light moves by less than a texel of the map, held to the baked shadow's own
  figures from status.md or an owner-given number. This is the test that owns the
  earlier complaint.
- **Shadow, acne and leaks.** No lit speckles on a lit floor (a count of pixels
  deviating from the unshadowed frame outside the shadow), and no light through a
  wall's corner.
- **Parallax, truth.** A CPU brute-force march of the same height field gives the texel a
  pixel should show; a sample of pixels across one face must show those texels. Fails
  with the offset zeroed.
- **Parallax, parallax.** Two eye positions a known distance apart shift a feature's
  position on a brick by the geometric amount, within a tolerance in pixels.
- **Parallax, quiet.** At distance the relief is gone and the frame equals the bump-only
  frame; a flat height map draws as no parallax; a face at a glancing angle shows no
  hole or smear of more than a pixel.
- **Hostile.** The existing `hostile_level` corpus and a forged height file through the
  GPU build, to errors and not a panic.
- **Seeded.** As M14 did, break each term by hand once (no bias, no normal offset, no
  fade, wrong step count) and record that its test fails.

## Costs

Targets on the dev machine at 3840x2160, High, over the 5.1 ms of today's frame with
readback: shadows 1.5 ms or less for two lights at 1024, parallax 1 ms or less on the
showcase, to be measured and recorded in `docs/perf/`. Software and the effects
switched off cost nothing more. Memory: two cube maps of 1024 at `R32Float` is 48 MB, so
the default is 512 at first, 12 MB, with 1024 behind the measurement.

## Risks

- **Looking like shadow maps.** The owner has rejected crawling and jagged shadow maps
  on the static light. Dynamic shadows move anyway, so crawl is less visible, but the
  soft kernel and the stability test are what stand between this and the complaint.
- **No software check.** These looks have no software twin, and parity is dropped, so
  only their own tests and the off-is-a-no-op check catch a bug in the shared shading.
- **Wasted work if the software fallback is used.** A machine that falls back to
  software shows a plainer picture. That is the decision, and the log says so.
- **Static casters make the biggest cost.** Decision 1 above; the cache of a still map is
  the lever.
- **Screenshots and scripts.** Headless runs default to software, so `--screenshot`
  pictures show neither effect unless `--renderer hardware` is given.
- **Voxel receivers.** Their dynamic light is a chunk-wide property; whether the same
  shadow lookup fits the pulled-vertex shader without a measurable cost is checked at M16.4.
