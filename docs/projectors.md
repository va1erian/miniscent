# Projectors (directional lights)

Status: M18.1 to M18.5 built, see [status.md](status.md#m18-projectors). Work items are M18 in
[work-items.md](work-items.md). Builds on [lightmaps.md](lightmaps.md) and
[dynamic-lights.md](dynamic-lights.md).

## Goal

A light that shines one way. A level author places a projector, aims it with a yaw and a
pitch, gives it a cone, and gets a pool of light with a soft rim on whatever the cone
reaches, shadowed like any lamp. It is how a level lights a doorway, a stage, or a
single statue without lighting the room.

## Decisions

- **A spot cone, not a projected picture.** A cone with a soft edge covers the uses above
  and costs one dot product in the baker. A projected texture (a gobo or cookie) is a
  later item and would add a material id and a UV frame to the entity; nothing here
  prevents it.
- **Baked, so both renderers draw it.** The lightmap holds a lamp's light for the level
  standing still. A projector is one more lamp for the baker, so the software renderer,
  the GPU, voxel surfaces and the editor's 3D view show it with no change of their own.
  This keeps to the two-renderer rule: nothing about it is in the simulation.
- **A new entity, `Entity::Projector`,** not fields on `Light`. `Light` is in every
  level; changing its record would break old files and every `match`. A projector is tag 8
  of the entity section and format version 6. Old readers refuse a version 6 file; this
  reader reads versions 1 to 6, and a version 5 file with a tag 8 entity is `BadEntityTag(8)`.
- **Direction is yaw and pitch,** radians, like the player start's yaw, so the editor's
  fields and the text format need no new idea. The cone has an inner half angle (full
  light) and an outer one (none), a smooth step between, `0 <= inner <= outer <= PI / 2`.
- **Reuses `PointLight`** with an `Option<Cone>`, so the baker's one `light_at` handles
  both. The cone is tested before the shadow ray, so a luxel outside the beam casts none:
  a projector costs less to bake than the lamp of its radius.
- **The bake key** adds a cone's numbers only when a light has one, so every cache made
  before projectors is still valid.

## Projected shadows

Walls shadow a projector in the bake. What moves (the ship, enemies, movers) shadows it on
the GPU through the static-lamp maps of M16.6, so a projector competes for those two maps
like any lamp. The map is a cube, so a projector spends a whole cube on a cone; a map that
covers only the cone is a later saving. The face shader multiplies the lamp's share by the
cone's smooth step (`cone_share` in `face.wgsl`, the same curve as `Cone::share`), so a
shadow in the rim of the beam takes only the light the bake put there. `Frame` grows by 32
bytes (`lamp_aims`, `lamp_edges`). Software draws no moving shadows, as before. Known waste: the two maps are picked by brightness
and reach alone, so a bright projector aimed away from every mover can hold a map that
shades nothing while a dimmer lamp goes without.

## Where a projector is not drawn

- **Models and actors** are lit by point lights on the CPU with no cone. A projector
  would light them from behind the beam, so they ignore projectors (`mesh_lighting`).
- **Scripts** cannot switch a projector (the game keeps no spec for it). Follow-up with
  the `kit` light API.
- **Dynamic lights** (shots, flashes) are point lights and unchanged.

## Work items

| ID | Item | Done when |
|---|---|---|
| M18.1 | World: `Entity::Projector`, format version 6, level text, validation | Round trip in binary and text; hostile and flipped-bit files return errors; a version 5 file still loads |
| M18.2 | Baker: `Cone`, `PointLight::cone`, cone test before the shadow ray, bake key | Light only inside the cone, a soft rim, a wall still shadows it, old keys unchanged |
| M18.3 | Renderers: the setup, the fallback lamp lighting, models and lamp maps skip projectors | A level with a projector draws lit inside the cone and unchanged outside, on software and on the GPU |
| M18.4 | Editor: Projector tool, panel fields, a cone drawn in the 2D views, selftest | Place, aim, undo; the selftest bakes a projector level and reads a lit and an unlit pixel |
| M18.5 | The showcase level gets projectors, and the new features it can show | Loads, compiles and keeps its frame cost; screenshot shows the pools |
