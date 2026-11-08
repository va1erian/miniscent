# Enemy designer: design

Status: accepted; M19.1 and M19.2 are built ([as built](#as-built)). Work items are [M19](work-items.md#m19-enemy-designer). It takes the
foundry in [enemies.md](enemies.md) (blueprints, the model path on both renderers) and makes it something a
level designer uses: enemies are designed in the editor, by generator, by hand or both, saved **in the level**,
and placed as real enemies that play and draw as designed. It replaces M15.4, M15.10 and the editor half of
M15.6, and drops the red horned ball (`Mesh::enemy`) altogether.

## As built

### M19.1

`crates/mscent-world/src/kinds.rs`, `format_kinds.rs` and `level_text/kinds.rs`, with these changes from the design below:

- **Stats** are 8 numbers with no flags: hp and damage as `u32`, then radius, speed, sight and attack range in world
  units, and cooldown and patience (`lose`) in **seconds**, so a kind does not depend on the step rate. The game
  turns them into steps in M19.2. A body radius is at most 64 world units.
- **Names** may hold `_` as well as `[a-z0-9-]`, since script kinds and labels already do (`drone`, `sec_drones`).
- **Cost** for a whole level is `MAX_LEVEL_MODEL_NS`, four times one model's, which is 1 s estimated, not 2.
- **An empty table is the stock table.** `Level::kinds` is empty for every older level, and writing one with no kinds
  writes no section, so old levels keep their bytes but for the version. `Level::kind_table` and `kind_count` give the
  table an enemy plays; `stock_kinds` reads `sentry.msb` and `dart.msb`, compiled into the world crate.
- **Enemies past the table** are refused for every level, stock ones included. No shipped level had one. The
  editor holds the Kind field inside the table, and Save already refuses a level the loader would.
- **Text** blocks take every stat by name, in any order, and the blueprint behind `|` exactly as its file has it.
  A blueprint's error names its line in the level text.

### M19.2

`crates/mscent-game/src/kind.rs` (`KindPlay`, `kinds_of`, `stats_of`) and `facing.rs`, with these changes from the
design below:

- **Building models at load moves to M19.3**, with the renderers that draw them. The simulation needs only stats
  and hit spheres, which reading gives, so the game does no build yet.
- **Stats in steps.** Times are rounded to whole 120 Hz steps. `stats_for(0)` and `stats_for(1)` now come from
  `KindStats::DRONE` and `HUNTER` and equal the old literals exactly, which a test holds.
- **Body radius** is the kind's radius or the reach of its farthest hit sphere, whichever is larger: about 3.7 world
  units for the stock mantis, so a mantis needs a room to match.
- **Every hit sphere is tested**, not only those of an enemy whose body sphere the shot meets first. Up to 8 an enemy
  is cheap, and it makes the nearest sphere win across enemies. Only the player's shots use hit spheres; an enemy's
  shot at another enemy (friendly against hostile) still aims at the body, as before.
- **Facing** is a `Quat` eased each step toward the target while attacking and otherwise toward where it moves. It is
  built from square roots alone, never a sine, since IEEE rounds a square root the same everywhere and a replay's hits
  depend on it. The stock table has no hit spheres, so for old levels facing changes nothing that is played.
- **Scripts.** `defineEnemy:base:` takes a kind's name, whose stats it copies as they stand, or a number below the
  level's kind count; the new kind wears its base's hit spheres.
- **Binary.** The software-only game went from 2,201,088 bytes (main, measured) to 2,281,472, 99% of its
  2.3 MB budget; the default build from 6,088,704 to 6,125,568. Most of it is the blueprint reader and writer, which
  the game now links to read a level's kinds. M19.3 links the builder too, so it has under 19 KB left before the budget
  has to be raised or something shrunk.

## The design in brief

| Question | Decision | Why |
|---|---|---|
| Where does a design live? | In the level, in a new **kinds** section: name, blueprint, stats | A level carries what it needs. No global id table to keep append-only, which was [enemies.md](enemies.md) open question 4 |
| How is it shared between levels? | A library, `assets/enemies/*.msb`, that the designer imports from and exports to, by copy | As materials are shared, but a level never changes because a library file did |
| What does `Entity::Enemy { kind }` mean? | An index into the level's kinds table | Same byte as now, so the entity record does not change |
| Old levels? | A level with no kinds section reads as two stock kinds, `drone` and `hunter`, with today's stats and one hit sphere each | Replays stay bit-exact; only the picture changes |
| What replaces the horned ball? | `drone` wears the stock sentry, `hunter` the stock dart, built into the game | Every enemy is a model, so there is one draw path for enemies, not two |
| Where do generators live? | In the editor, `apps/editor/src/foundry/`, beside the voxel generators | The game reads pieces, never seeds; plans cost the game binary nothing |
| What does a generator make? | Blueprint text, read back through `model::read` | A generated design passes the same checks as a file from disk, so the editor cannot make what the game refuses |
| Can a person edit the result? | Yes: genes with locks, skins, stats, hit spheres, and the blueprint's text itself | Generators give the start; a person gives it a role |
| What does the simulation read? | Stats and hit spheres, as [enemies.md](enemies.md) planned | The mesh can change without changing play |

## Data

### Kinds section

Level format version 7 adds section `6`, **kinds**. The entity section is unchanged.

```text
count u16                        <= MAX_LEVEL_KINDS (32)
kind*:
  name     u8 len, bytes         1..=32 bytes of [a-z0-9_-], unique in the level
  stats    hp u32, radius, speed, sight, attack range f32, damage u32, cooldown s, lose s f32
  text     u32 len, bytes        a blueprint, <= model::MAX_FILE (64 KiB)
```

- **Read as untrusted.** Each blueprint goes through `model::read`, so every limit in
  [enemies.md](enemies.md#hostile-input) holds. Stats are finite and inside the ranges `defineEnemy:` already
  enforces (`MAX_STAT`). A name that repeats is an error.
- **Cost for the whole level.** The sum of `estimated_ns` over its kinds may not pass `MAX_LEVEL_MODEL_NS`, four
  times `MAX_MODEL_NS`, 1 s estimated: 32 cheap kinds load, 32 costly ones are refused before any is built.
  Checked from the file alone, as recipes are.
- **Entity kinds out of range.** An `Entity::Enemy` whose `kind` is past the table is a load error naming the entity,
  not a silent drone. The writer never makes one: deleting a kind still in use is refused in the editor.
- **Missing section.** Versions 1 to 6, and 7 written with no kinds, read as the stock table: `0 drone`, `1 hunter`,
  with today's `stats_for` numbers and one hit sphere of the body's radius. So `Enemy::new(i, pos, kind)` and every
  replay play exactly as now.
- **Text form.** `level_text` gains a block, so a level stays diffable:

  ```text
  kind hornet hp=40 radius=0.75 speed=9 sight=40 attack=8 damage=4 cooldown=0.4 lose=3
  | miniscent-blueprint 1
  | name hornet
  | ...
  end
  ```

### What a kind holds

| Field | Read by | Notes |
|---|---|---|
| Name | Scripts, the editor | `spawn: 'hornet' at:` finds level kinds as it finds script ones |
| Blueprint | Renderers | Its `plan` line records the generator and seed, for provenance only |
| Hit spheres (in the blueprint) | Simulation | Up to 8, in model units, turned by facing |
| Body radius | Simulation | Bounding radius of the hit spheres at the blueprint's scale, or the stat if larger. Not taken from the mesh, so replays never depend on one |
| Stats | Simulation | `EnemyStats` as now |

Blueprint scale is in the blueprint, so the 0.25 of the stock designs carries through; the designer shows the
size in world units next to the player's.

## Game

- **Load.** `Level` carries `kinds`. The game builds each kind's model once at load into the renderer's
  `ModelTable` and keeps `ModelId` per kind. A build that fails (over vertices, CSG error) is a load error naming
  the kind. Stock kinds come from `include_str!` of `sentry.msb` and `dart.msb`, so a level that names neither
  costs nothing extra, and the two stock files gain a unit test that they build.
- **Enemy.** Gains `facing: Quat`, eased toward velocity, or toward its target while attacking, each step. It is
  simulation state, so replays record it by construction. `Enemy::sphere` stays the body sphere; `hitscan`
  tests a kind's hit spheres only when the body sphere is hit, as [enemies.md](enemies.md#play) planned. Stock kinds
  have one hit sphere equal to the body, so their results do not change.
- **Scripts.** `defineEnemy: 'x' base: 'hornet'` takes a kind name as well as 0 and 1, and `spawn:at:tag:` finds
  level kinds by name. Nothing else changes.
- **Drawing.** `ActorKind::Enemy` carries its `ModelId` and facing, and both renderers send it through the model
  path they already have (M15.2 software, M15.11 GPU). `Mesh::enemy` and its tests are deleted. Light is the
  model path's stopgap until M15.3; that is accepted here, as the GPU may look better and software has only to look
  good on LazyOS ([hardware-renderer.md](hardware-renderer.md), Two renderers).
- **Friendly enemies** draw their kind's model too. Until M15.3's instance tint, a friendly one keeps a small green
  ring under it from the existing mesh code, so play can still tell sides apart; the green ball goes.
- **Binary.** The build path (`model::build`, CSG and the mesher for models) becomes linked. M15.2 left the game
  at 2.03 to 2.05 MB of 2.2. M19.2 measures it and records the number; if it passes the budget, the fallback is
  to ship each kind's built mesh in the level instead, as [enemies.md](enemies.md#alternatives-rejected) priced it.

## Generators in the editor

`apps/editor/src/foundry/`, never linked by the game.

```text
foundry/
  mod.rs        Plan trait, registry of plans, Genes
  genes.rs      gene kinds: choice, count, symmetry, proportion (low/mid/high), skin
  emit.rs       a blueprint builder that writes text, then model::read on it
  gates.rs      one object, triangle budget, bounds, hit cover
  fit.rs        hit spheres fitted to a built model
  plans/        sentry.rs, dart.rs, mantis.rs first; one file per plan
```

- **A plan** is a pure function `(seed, &Genes) -> Blueprint text`, as the voxel generators are pure from seed to
  `Plan`. It declares its genes, each with a name, its choices and a default. Genes are drawn from the seed first,
  then any the person has locked override the draw, so locking one and rolling breeds around it.
- **Discrete genes first**, for the reason [enemies.md](enemies.md#2-genes-discrete-choices-first) found: wing layout,
  engine layout, limb count and symmetry change the silhouette; numbers in ranges give siblings.
- **Emit through the reader.** `emit.rs` writes text and `model::read`s it. A plan that produces an invalid design
  is a bug the plan's tests catch over 500 seeds; in the editor it shows as a failed candidate, never a crash.
- **Gates** run after a build on the worker: `solid_parts` is one per part, triangles under the class budget,
  bounds inside the class box, and hit spheres cover at least 80% of a 32x32 silhouette from six sides. A
  candidate that fails one is shown greyed with the reason; the person may still take it.
- **Hit spheres** are fitted by `fit.rs` from the built mesh, greedily, up to 8. The person can move, resize, add or
  drop them afterwards.
- **First plans.** Three, rewritten from the hand-made stock files so their output looks like them at some seed:
  sentry (drone), dart (ship), mantis (alien). The rest of the eighteen in [enemies.md](enemies.md#1-body-plans-different-code-not-different-numbers)
  are added one plan per change later. The diversity archive (MAP-Elites) is left out of the first version: a
  sheet of eight seeds and locks is the loop a person uses; the archive is for the headless `--foundry` run.

## The Enemy Designer workspace

Opened from a new **Enemies** menu beside **Texture** (**Enemy Designer...**), or by double-clicking a placed enemy, which opens on its kind. Built as
the Texture Studio is: a workspace that replaces the views, its own undo, a worker with cancel, a tick that never
waits, and nothing of the level touched until **Apply**.

```text
+-- kinds in level -+------------- candidates (8 seeds) -------------+-- design -----------------+
| drone      x12    | [  ] [  ] [##] [  ] [  ] [  ] [  ] [  ]         | plan   [dart      v]      |
| hunter      x3    |  1187  1188  1189 ...    tris / gate per thumb  | seed   [1189] [Roll]      |
| > hornet    x0    +-------------------------------------------------+ wings  [swept   v] [lock] |
|                   |                                                 | engines[twin    v] [lock] |
| + New from plan   |   orbiting view, lit as in the game,            | length [mid     v] [ ]    |
| + Import library  |   player ship beside it for size,               | skins  hull [plating v]   |
|   Duplicate       |   hit spheres as wire, part motion running      |        glow [glass   v]   |
|   Export library  |                                                 | stats  hp 40  speed 9 ... |
|   Delete          |   tris 908 / 2000  build 5.7 ms  est 11 ms      | hits   [Fit] [+] [-]      |
|                   |   gates: one object ok, cover 86% ok            | [Design] [Blueprint text] |
+-------------------+-------------------------------------------------+---------------------------+
                                             [Revert]  [Apply to level]  [Back to level]
```

- **Kinds list** shows how many placed enemies use each. Delete is refused while any do, and says how many.
- **Roll** runs the plan over the next eight seeds on the worker and fills the sheet; clicking one makes it the design.
  Each thumbnail is the model drawn by the software model path at fighting distance, so it shows what the player
  will see and needs no GPU.
- **Design tab:** genes, skins (role to material, from the level's material table, so Texture Studio materials appear
  here), stats, hit spheres. Changing a gene rebuilds; dragging a stat does not.
- **Blueprint text tab:** the blueprint as text, with the script pane's editor and errors from `model::read` by line.
  Editing it by hand detaches the design from its plan: the plan line stays as provenance, genes are greyed, and
  Roll asks first, since it would replace the hand edits.
- **Preview** orbits the model through the same renderer the 3D pane uses, at its world size, beside the player ship.
  Parts move once M15.5 lands; until then they stand still and the panel says so.
- **Apply** writes the edited kinds into the level as one undoable level command, so placed enemies change at
  once in the 3D view and Ctrl+Z in the level takes the whole Apply back. Instant Play started from the workspace
  applies first, as Play from the Texture Studio does now.
- **Library.** Import copies a `.msb` and a stats line from `assets/enemies/` into the level; Export writes them
  back, atomically, with the Texture Studio's `write_atomic`. The library adds a stats header to each `.msb` as a
  comment line the blueprint reader already skips: `# stats hp=40 speed=9 ...`.

### Placing

- The Enemy tool's panel replaces the numeric **Kind** field with a dropdown of the level's kinds by name, each with
  its thumbnail. New enemies take the last kind used.
- The 3D pane draws each enemy as its model, facing +X, as the game will at its first step.
- The 2D views draw the body sphere's circle and the kind's name, and warn, as [enemies.md](enemies.md#play) asked,
  when the body cannot fit where it is placed.

## Hostile input and limits

| Limit | Value | Where |
|---|---|---|
| Kinds in a level | 32 | `MAX_LEVEL_KINDS`, world |
| Blueprint per kind | 64 KiB, and every limit of `model::read` | world |
| Estimated build, whole level | 4 x `MAX_MODEL_NS` | world, before any build |
| Name | 1 to 32 bytes, `[a-z0-9_-]`, unique | world |
| Stats | finite, 0 to `MAX_STAT`, radius above zero | world, the same rule as scripts |
| Entity kind | below the table's length | world |

Tests in the manner of `voxel/wire_tests.rs`: bit flips and truncation at every byte of a level with three kinds,
each limit one past, a duplicate name, an entity past the table, and a blueprint that reads but costs too much.

## Glitches designed out

| Glitch | Prevented by | Test that reads real output |
|---|---|---|
| Old replays change | Stock kinds have one hit sphere of today's radius and today's stats | Every replay in `tests/` bit-exact before and after |
| Editor makes what the game refuses | Plans emit through `model::read`; Apply checks the level's cost as the reader will | Saving every candidate of 500 seeds per plan and reloading it through `format::read` |
| A level changes when the library does | Import copies | Edit a library file; the level's bytes do not change |
| An enemy points at a deleted kind | Delete refused while in use; reader refuses out of range | Editor test; hostile file test |
| Enemy too big for its room | Body radius from hit spheres; 2D warning | `fuzz.rs` with the stock and generated radii |
| Thumbnails stall the UI | Worker, cancel on a new roll | `tick_costs` worst tick during a roll |

## Work items

| ID | Item | Deps | Size | Done when |
|---|---|---|---|---|
| M19.1 | World: kinds section (format 7), text block, stock table for old levels, limits, level cost | M15.1 | M | Round trip binary and text; bit-flip and truncation fuzz; every limit; old levels read the stock table |
| M19.2 | Game: kinds from the level, build at load, facing, hit spheres, scripts by name; binary size measured | M19.1 | M | Replays bit-exact; a hand-written kind plays end to end through `--test`; size recorded against 2.2 MB |
| M19.3 | Renderers: enemies drawn as their kind's model on both; `Mesh::enemy` deleted; friendly ring | M19.2, M15.11 | M | Frame tests on both find the model's pixels where the ball was; a showcase PNG looked at |
| M19.4 | Editor placing: kind dropdown with thumbnails, model in the 3D pane, name and warning in 2D | M19.1, M19.3 | S | Selftest places two kinds, saves, reloads, and both draw |
| M19.5 | Foundry core in the editor: plans, genes and locks, emit, gates, fit; sentry, dart, mantis plans | M19.1 | L | Same seed same text; 500 seeds per plan read back; gates reject planted failures |
| M19.6 | Enemy Designer workspace: kinds list, sheet, preview, design and text tabs, Apply, library | M19.4, M19.5 | L | `--selftest` rolls, locks a gene, edits text, applies, saves, reloads and plays the kind; `tick_costs` held |
| M19.7 | `--foundry plan --seeds N --out dir` headless contact sheet | M19.5 | S | Writes a sheet PNG and the gate table for 2000 seeds |

M19.1 to M19.3 is the first visible result: the horned ball gone and stock levels showing sentries and darts.
M19.5 can run beside M19.2 and M19.3, as it needs only the format.

## Alternatives rejected

| Alternative | Why not |
|---|---|
| A global `enemies.txt` with append-only ids, as [enemies.md](enemies.md#blueprint) planned | Levels then depend on a file outside them, and ids can never be reused. Materials already live with this, and it is the open question both docs list |
| Levels store a library name only | A library edit would change shipped levels, and a level could not be sent alone |
| Store built meshes in the level | Tens of KB a kind, where a blueprint is 1 to 2 KB and the game links the builder anyway. Kept as the fallback if the binary passes its budget |
| Keep the horned ball for kinds 0 and 1 | Two draw paths for enemies forever, and the old look in every stock level |
| Generators in the game | As [enemies.md](enemies.md#alternatives-rejected): binary budget, and untested designs reaching players |
| Open as level, sculpt, bring back | Needs a delta format ([enemies.md](enemies.md#open-questions), question 2). The text tab covers hand edits for now |

## Open questions

1. **Stock table names.** Old levels' kinds become `drone` and `hunter`, the names scripts already use. Should
   `hunter` wear the dart or the mantis? I took the dart: a hunter is fast, and the dart reads fast.
2. **Per-instance variety.** [enemies.md](enemies.md#play) gives each instance up to four variants and a tint by its
   `Rng`. Left out here; a kind could later hold variants as seeds of its plan.
3. **Reference machine.** Load cost of 32 kinds is about 0.3 s on the dev machine by the M15.1 table, and an
   estimated 1.5 s on the i3-7100U, not measured.
