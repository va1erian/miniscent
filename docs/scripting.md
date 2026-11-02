# Level scripting: design

Status: accepted. The language and VM exist (M10.4, `crates/mscent-script`); no game API is bound to them yet. The work is M10 in [work-items.md](work-items.md). Why this
language, with measurements: [scripting-language.md](scripting-language.md).

## The short version

- **One script per level**, `foo.st` beside `foo.msl`. It holds all of the level's logic, and it reads
  top to bottom.
- **No logic entities, ever.** A level file holds places, shapes and solids, each with a name. It has
  no relays, counters, timers, outputs or target fields. That rule is what prevents the soup.
- **A small Smalltalk-style language in the repo**, with no dependencies, compiled at load to bytecode.
  Rust owns physics and invariants; a script can only ask.
- **One mover primitive** covers doors, trapdoors, lifts and crushers: a named brush group with named
  poses.
- **No rebuild for an agent:** `--check` validates a script, `--test` plays a level headlessly with
  assertions, `--trace` writes what happened as JSON lines, and a running game reloads on save.

The largest piece of work is the movers, not the language. The engine has no moving geometry today:
collision, rendering and light all assume a single static BSP.

## What it looks like

Every example here ran on the prototype against a stub host, handlers included. The stub logs commands
instead of simulating, so this proves the language and the timing of waits, not gameplay.

Shoot several things to open a door:

```smalltalk
vault := mover: 'vault_door'.
vault pose: 'open' move: (x: 0 y: 0 z: 4).

(markers: 'vault_switch') do: [:m | spawn: 'switch' at: m tag: 'vault_switch'].

switches := tagged: 'vault_switch'.
switches onEachDestroyed: [say: switches alive , ' left'].
switches onAllDestroyed: [
    vault moveTo: 'open' in: 1.5.
    say: 'Vault unlocked'.
].
```

Pick up a keycard. `autoDoor:` is ordinary script in the shared library, not an engine feature:

```smalltalk
card := spawn: 'keycard' at: (marker: 'red_key') tag: 'key'.
card onPickup: [
    player give: 'red'.
    say: 'Red keycard'.
].

autoDoor: 'red_door' travel: (x: 0 y: 0 z: 3.5) key: 'red'.
autoDoor: 'side_door' travel: (x: 0 y: 2 z: 0) key: nil.
```

```smalltalk
"assets/scripts/lib/doors.st: opens as the player arrives, closes after they leave."
define autoDoor: name travel: travel key: key [
    door := mover: name.
    door pose: 'open' move: travel.
    area := zone: name , '_area'.
    area onEnter: [
        (key isNil or: [player has: key])
            ifTrue: [door moveTo: 'open' in: 0.6]
            ifFalse: [say: 'Needs the ' , key , ' keycard'].
    ].
    area onExit: [door moveTo: 'closed' in: 0.8].
]
```

A door, a hinged trapdoor, a lift and a crusher are one primitive:

```smalltalk
trap := mover: 'trapdoor'.
trap pose: 'open' turn: (x: 1 y: 0 z: 0) degrees: 90 about: (marker: 'trap_hinge').
(zone: 'trap_plate') onEnter: [trap moveTo: 'open' in: 0.4].

lift := mover: 'lift'.
lift pose: 'top' move: (x: 0 y: 0 z: 12).
lift whenBlocked: 'reverse'.
every: 6 do: [
    lift moveTo: (lift poseNow = 'top' ifTrue: ['closed'] ifFalse: ['top']) in: 2.5.
].

crusher := mover: 'crusher'.
crusher pose: 'down' move: (x: 0 y: 0 z: -3).
crusher whenBlocked: 'crush' damage: 40.
fork: [
    [true] whileTrue: [
        crusher moveTo: 'down' in: 0.5.
        waitArrived: crusher.
        wait: 1.
        crusher moveTo: 'closed' in: 2.
        waitArrived: crusher.
        wait: 1.
    ].
].
```

Spawn waves. A handler can pause, so a sequence is just lines of code:

```smalltalk
wayOut := mover: 'hall_exit'.
wayOut pose: 'open' move: (x: 0 y: 0 z: 3.5).
entry := mover: 'hall_entry'.
entry pose: 'open' move: (x: 0 y: 0 z: 3.5).
entry moveTo: 'open' in: 0.

(zone: 'hall') onceEnter: [
    entry moveTo: 'closed' in: 0.5.
    wait: 1.
    spawnAll: 'drone' at: (markers: 'hall_wave1') tag: 'wave1'.
    waitDestroyed: (tagged: 'wave1').
    wait: 2.
    player hp < 50 ifTrue: [
        say: 'Sector clear, repairs online'.
        player heal: 50.
    ].
    spawnAll: 'hunter' at: (markers: 'hall_wave2') tag: 'wave2'.
    waitDestroyed: (tagged: 'wave2').
    wayOut moveTo: 'open' in: 1.5.
].
```

Hack a terminal to turn enemies friendly, and tune a new enemy. The hold time and progress bar are
engine behaviour; the consequence is script:

```smalltalk
kind := defineProp: 'console'.
kind mesh: 'terminal'.
kind radius: 1.5.

sentry := defineEnemy: 'sentry' base: 0.
sentry hp: 80.
sentry speed: 0.

terminal := spawn: 'console' at: (marker: 'sec_terminal') tag: 'term'.
terminal onUse: [
    (tagged: 'sec_drones') setFaction: 'friendly'.
    (tagged: 'sec_turrets') disable.
    say: 'Security override: drones are on your side'.
] hold: 3.
```

Test it headlessly. A test is the same language, loaded after the level script, with a few extra
functions. `wait:` is how it lets the game run:

```smalltalk
vault := mover: 'vault_door'.
assert: vault poseNow = 'closed' because: 'vault starts closed'.
(tagged: 'vault_switch') members do: [:s |
    teleport: s distance: 8.
    aimAt: s.
    fireFor: 0.8.
    wait: 0.1.
].
wait: 2.
assert: vault poseNow = 'open' because: 'vault opens once every switch is destroyed'.
shot: 'vault-open.png'.
```

## Three layers

| Layer | Lives in | Holds | Changes need |
|---|---|---|---|
| Places | `.msl` | Named brush groups, markers, zones, enemies | Editor, or `--pack-level` |
| Kit | `mscent-game` (Rust) | Mover physics, props, pickups, zone tests, factions | A rebuild |
| Logic | `foo.st`, `assets/scripts/lib/*.st` | What happens when | Saving the file |

The test for where something goes: if breaking it could put a sphere inside a wall or stall a frame,
it is kit. If it is a decision about this level or this mechanic, it is logic.

### Rules that keep the soup out

1. The level format has no field that refers to another thing. Names point from script to level, never
   from level to level.
2. A repeated pattern becomes a library function, not a new entity kind.
3. The editor has no outputs panel. It shows names and tags, and a read-only list of which lines of the
   script use the selected thing.
4. Common "wait for N things" logic is a built-in (`onAllDestroyed:`, `waitDestroyed:`), so nobody
   hand-writes a counter.

## Level data

Entity is `Copy` and stays so: names live in a side table, not on the entity.

- `Entity::Marker { pos, yaw }`: a named point.
- `Entity::Zone { pos, half }`: a box the player enters and leaves.
- `Level.labels`: `{ entity, name, tags }`, with an optional name, since a marker may only need a tag.
- `Level.mover_groups`: `{ name, tags, brushes }`, indices into the brush list. `Level::static_brushes()`
  is what `compile` and `extract_faces` receive, so a mover never reaches the CSG.

These go in a new format section under version 3 (`format_labels.rs`, `labels.rs`). The reader validates
them like everything else: identifiers only (`[a-z0-9_]`, at most 32 bytes), capped counts, no duplicate
names, no brush in two groups, mover brushes must be `Op::Add`, and indices in range. The editor holds
them by stable id, so reordering and deleting cannot point a name at the wrong thing.

Props, pickups and waves are spawned from markers by the script. So an agent can add a keycard without
touching the level file, and the entity list stays nearly the same size as today.

`LoadError::NoGoal` allows a level with a script, which can win through `win`.

## Movers

A mover is a group of convex `Add` brushes, excluded from the CSG, with a rigid pose (translation and
rotation). `closed` is the authored pose. Other poses are defined in script, and `moveTo:in:` eases
between two poses over a time. Quake's `func_door`, `func_plat` and `func_train`, and Source's
`func_movelinear` and `func_door_rotating`, all reduce to this.

### Collision

`Cell` (`crates/mscent-collide/src/cell.rs`) already does an exact sphere sweep against a convex plane
set, including rounded edges and vertices. Movers reuse it:

- Each brush becomes a `Cell`, built once at load. The current sweep rebuilds cells on demand, and doing
  that for movers on every sweep would be wasted work.
- A new `Solids { bsp, movers }` replaces `&Bsp` in `sweep_sphere` and `slide_move`, with `From<&Bsp>`
  for callers that have no movers. A sweep takes the earliest contact across the BSP and each mover
  whose bounds it touches. It transforms the segment into the mover's frame; a sphere's radius does not
  change under a rigid transform, so the cell code is untouched.
- Line of sight and hitscan go through the same path, so a closed door stops shots and sight. `hitscan`
  also reports which mover it struck, which makes a mover shootable with no extra code.

### Pushing

Movers advance after the player and enemies in each fixed step. A body's previous position is set when
it moves, so a push lands between its two interpolated positions and draws smoothly:

1. Compute the trial pose for this step.
2. For each body near the swept bounds, transform its position into the mover's frame at the old and
   new poses. The line between them is the body's path relative to the mover. Sweep it against the
   cells.
3. On contact, push the body the rest of the way with `slide_move` against everything except this
   mover. Its velocity is not changed. A body wedged at an angle, between a wall and a sloped mover, gets
   a few more rounds of this, so it slides out instead of stopping the mover.
4. If it still overlaps the mover it is squeezed, and the mover's policy applies: `hold` stops at the last
   clear pose, `reverse` goes back, and `crush` holds and deals damage per second until the body dies.
   `reverse` is the default. Bodies that have been pushed are restored, so a blocked step leaves nothing
   half done.

A step is checked along a straight chord, even when the mover turns. The ends of that chord are exact, so
no body ends up inside a mover however fast it spins. The fuzz test holds at 1.5 radians per step, and a
first version's speed cap had no test that could show a need for it, so there is none. Only the path
between the ends is approximate, which at worst lets a blade's tip pass a body without touching it.

If a body's centre is inside a mover, the sweep ignores that brush rather than reporting `start_solid`, so
a bad spawn cannot freeze the player forever. A body only slightly overlapping is still pushed out, or a
rotating door could sink through it. The BSP stays strict.

`tests/fuzz_movers.rs` extends the existing guarantee, with oracles that share no code with the collision:
no sphere ends inside the world or a mover, and none crosses either in flight. Each of six deliberate bugs
in the collision code fails the mover tests, so they can fail.

### Rendering and light

- Mover faces come from `Brush::faces()`, drawn before the world, so the z-buffer saves shading whatever a
  door covers. A mover wholly behind the eye is skipped; off-screen faces are left to the clipper.
- Faces join the lightmap's `LightFace` list at the authored pose, so `input_key` covers them and the
  bake cache keeps working. Lighting a face uses face-space UVs, so only the polygon is transformed at
  draw time and the light rides with the door. Specular needs the eye in the mover's frame. Positions
  interpolate with the same `alpha` the enemies use.
- Limits, as in Quake: **movers do not cast baked shadows**, so light passes through a closed door, and
  a door keeps the light it was baked with as it moves. Author lights with that in mind.
- A closed door does not cull what is behind it, so it costs what an open doorway costs today.
- Lights cannot be switched at runtime. That would need separate light layers in the bake, and is out of
  scope here.

## The language

The whole of it, in the form agents were given in the test in
[scripting-language.md](scripting-language.md):

```smalltalk
x := 3.                        "assignment; the first one declares the variable"
door moveTo: 'open' in: 1.5.   "keyword message: receiver, then name: arg name: arg"
say: 'hello'.                  "no receiver: a global function"
player hp.                     "unary message; a bare word that is no variable is a global function"
a + b * c.                     "binary runs strictly left to right: (a + b) * c"
'n = ' , 3.                    "comma concatenates and converts to text"
{1. 2. 3}.                     "array, 1-indexed: a at: 1, a size, a add: x"
[:x | x + 1].                  "block; call it with b value: 1. It can change outer variables"
cond ifTrue: [ ] ifFalse: [ ]. "also ifTrue:, ifFalse:, and:, or:, not, isNil"
[cond] whileTrue: [ ].         "also whileFalse:, n timesRepeat: [ ], a do: [:x | ]"
define name: a other: b [ ^ a + b ]   "function; it sees top-level variables"
define arr first [ ^ arr at: 1 ]      "with a receiver: the first word names it"
wait: 2.                       "pause this handler only"
fork: [ ].                     "run a block alongside"
```

Values are nil, true, false, numbers (`f32`), strings, arrays, blocks and game handles. Precedence is
unary, then binary, then keyword. There are no classes, modules, exceptions, maps or inheritance.
Functions and messages share one flat namespace, and new ones come from `define`. A name defined twice, or one that clashes with a game function or a language message, is a load error. A function defined with a receiver (`define arr first`) must be sent to one, and a plain function (`define autoDoor: ...`) must not be.

`^` returns from the `define` it was written in. Outside any `define`, as in a handler written at the top level, it ends that handler, so it works as an early exit. A block that outlives its `define` has nothing to return to, so `^` there is a runtime error.

**The rule that keeps it small:** no new syntax or language feature unless a level cannot be written
without it. A missing capability is usually a new game function.

### Runtime

- **Crate.** `mscent-script` holds the language and VM and knows nothing about the game. It depends on
  nothing and is generic over a host type. `mscent-game` depends on it and binds the API. There is no
  `dyn` boundary.
- **Pipeline.** Lexer, parser, AST, then bytecode. `ifTrue:` and `whileTrue:` with literal blocks compile
  to jumps, as in Smalltalk. Everything else is a send, resolved at load to a table entry.
- **State is plain data.** A slot table with checked, generation-tagged references, precise mark and
  sweep at step boundaries only, frames in a `Vec`. No `Rc`, no `unsafe`, no Rust recursion while
  running. So `Game` keeps its `Clone`, and a task parks by leaving the run loop.
- **Natives** are `fn(&mut Host, &mut Heap, Value, &[Value]) -> Result<Flow, String>`, registered from one
  table that also feeds `--script-api` and the load-time checks. `wait:` and `fork:` belong to the VM, not the
  table, and `do:`, `timesRepeat:`, `waitUntil:`, `every:do:` and `after:do:` are the crate's own prelude, loaded
  first as `<prelude>`. The game library must not redefine them.

### Where it runs in a step

```
player -> fire -> enemies -> movers -> zones
    -> start handlers for this step's events -> run due tasks -> win and lose checks
```

- **`Game` is the host.** A game function reads and changes the game directly, so a bad request is an
  error on the calling line, and `spawn:` then `alive` on the next line agree. There is no command queue
  and no snapshot. Handlers run one after another, each seeing what the last one did.
- **No same-step chains.** A move or a kill a handler causes raises its event on a later step, so a
  script cannot loop inside one step.
- **Budget.** At most 20,000 operations per task run, and 16 task runs per step. A task that was due but
  over the cap goes to the front of the next step's queue, so none starves. Worst case is about 0.6 ms on the dev machine, reached only by a hostile script. An idle step
  runs no script. The budget counts operations, not time, so it replays identically.

### API surface

| Area | Calls |
|---|---|
| Lookup | `mover:`, `zone:`, `marker:`, `markers:`, `enemy:`, `tagged:`, `player` |
| Mover | `pose:move:`, `pose:turn:degrees:about:`, `moveTo:in:`, `poseNow`, `moving`, `whenBlocked:`, `whenBlocked:damage:`, `onArrived:`, `onHit:` |
| Zone | `onEnter:`, `onceEnter:`, `onExit:`, `hasPlayer` |
| Group or one enemy | `alive`, `onEachDestroyed:`, `enable`, `disable`; a group also `members`, `onAllDestroyed:`, `setFaction:` |
| Spawned thing | `onPickup:`, `onUse:hold:`, `onHit:`, `onDestroyed:`, `destroy` |
| Spawning | `spawn:at:tag:`, `spawnAll:at:tag:` |
| Tuning | `defineProp:`, `defineEnemy:base:`, then a setter per field: `hp:`, `speed:`, `radius:`, `damage:`, `cooldown:`, `sightRange:`, `attackRange:`; `mesh:` on a prop |
| Player | `give:`, `has:`, `hp`, `heal:`, `pos` |
| Game | `say:`, `win`, `lose`, `randomFrom:to:`, `x:y:z:` |
| Time | `wait:`, `fork:`; in the library: `waitUntil:`, `waitArrived:`, `waitDestroyed:`, `every:do:`, `after:do:` |
| Test only | `assert:because:`, `teleport:distance:`, `flyTo:`, `aimAt:`, `fireFor:`, `shot:`, `phase` |

A handler that takes a parameter is given what its event was about: `[:enemy | ... ]` for a death, the
mover for `onArrived:` and `onHit:`, the player for a zone.

`markers:` takes a tag or the start of a name. Finding none is an error, because a typo there would
otherwise spawn nothing and wait forever. `tagged:` accepts any name, since a tag can come into being with
a spawn, so it has no load-time check.

`spawn:at:tag:` takes any enemy kind (`drone`, `hunter`, or from `defineEnemy:base:`) or prop kind (`switch`, `keycard`, `terminal`, `crate`, or from `defineProp:`). A name is one kind or the other. A prop does only what its script asks: it can be shot if it has health or an `onHit:`, picked up if it has an `onPickup:`, used if it has an `onUse:hold:`. Props are not solid. `say:` shows on the HUD and on the console.

`defineEnemy:base:` setters write the fields of `EnemyStats`, so most tuning needs no rebuild. A new AI
state does need one: `enemy.rs` stays Rust, because it runs at 120 Hz for every enemy and sight rays are
expensive. Scripts choose behaviour; they do not run per-step AI.

Two kit additions come with this surface. A **use key** (`G`, since `E` is roll and `F` is thrust) adds
`Input::use_key`. **Factions** let an enemy go after the nearest opponent within its sight, which is a search
per enemy per step, paid only once a script has made one friendly.

### Determinism and safety

- **Replay holds.** No clock is exposed, one seeded generator sits behind `randomFrom:to:`, and budgets
  count operations. A test runs a level twice and compares traces byte for byte.
- **Scripts are untrusted**, since they ship with levels. The language has no file, network or clock
  access to remove. Limits: a 1 MiB file cap, nesting depth 64, call depth 128, a 4 MiB heap counted in bytes (32 bytes
  of overhead an object), strings and arrays of at most 4,096, 128 live tasks, 65,535 constants, selectors and
  functions, 1,024 variables in one scope, 1,024 handlers, 256 tags, 64 enemy kinds, and 256 spawned enemies alive at once, 4,096 in a run. `cargo fuzz` feeds hostile text through
  compile and run.
- **Every command is validated in Rust.** An unknown pose, a non-finite vector or a position inside solid
  is a script error, not a game state. Arithmetic that gives a non-finite number is an error.
- **Failing closed.**

| When | What happens |
|---|---|
| Parse error, unknown selector or unknown literal name at load | The level does not start. `LoadError::Script` lists every problem the compiler finds, with file, line and closest match; a syntax error ends that one file's check at its first |
| Compile error on hot reload | Old script keeps running, error shown on the HUD |
| Runtime error in a task | Logged with file and line, shown on the HUD, that task ended and its handler disabled for the run. A `--test` run fails |

Selectors are a closed set and level names are usually string literals, so a typo fails at load even
inside a handler that never fires. A name built at run time, such as `zone: name , '_area'`, is checked
when it runs. So is a message sent to the wrong kind of value, such as `hp` to a mover.

## The agent loop

The goal is a full iteration in seconds, with evidence, and no `cargo build`.

```bash
miniscent --level assets/levels/foundry.msl --check
```

Compiles the script, runs its top level against the level, and prints an inventory: every mover, zone,
marker and tag, whether the script uses it, and every error. Exit code is non-zero on any problem. An
unused name and a variable assigned but never read are warnings, which catches typos in the level and in
assignments.

```bash
miniscent --level assets/levels/foundry.msl --test assets/levels/foundry.test.st --trace out.jsonl
```

Runs the level with no window as fast as the machine allows. Rendering happens only for `shot:`. The test
is one more task, so its `wait:` is what advances the game, and the run ends when it finishes. Tests aim
with `aimAt:` and `teleport:distance:` rather than the mouse deltas in `flythrough.txt`, which break
whenever geometry moves.

The trace is one JSON object per line:

```json
{"t":3.250,"ev":"destroyed","who":"enemy_2"}
{"t":3.250,"ev":"move_to","mover":"vault_door","pose":"open"}
{"t":3.250,"say":"Vault unlocked"}
{"t":4.750,"ev":"arrived","mover":"vault_door","pose":"open"}
{"t":5.000,"check":true,"because":"the vault opens"}
{"t":9.010,"err":"foundry.st:41: this message is for a mover"}
```

```bash
miniscent --script-api
```

Prints every game function and the language summary above, from the table that registers them, so it
cannot drift from the code.

```bash
miniscent --dump-level foundry.msl > foundry.level.txt
```

```bash
miniscent --pack-level foundry.level.txt foundry.msl
```

Levels are binary, and `LevelBuilder` needs a rebuild, so without this an agent cannot add a marker or a
door. The text form has one line per brush, entity and name, and goes through the same validation as the
binary reader.

**Hot reload.** While playing, in the game or the editor, the host checks the script's modified time four
times a second. On a change it compiles, then restarts the level on the new script if that succeeded.
State is not preserved, so a reload behaves exactly like a fresh start. Geometry is untouched, so there
is no re-bake.

**Editor.** The script path is per level. Brushes get a "make mover" command and things get name and tag
fields. The script pane (F6) edits the file with `xui-code-editor`: colours, completion that knows the
level's names and what a receiver understands, and a problem list checked against the level as you type,
the way `--check` does. F5 saves the script and plays it. The file is still plain text, and a change made in
another program is followed, or offered as a choice if there are edits in the way. This replaces the earlier
decision to have no script editor: placing a door and then leaving the program to wire it was the weak
step in the loop.

**Basic entities.** Script > Add entity places a door, lift, wave and so on. Each is ordinary level content
(named brushes, zones and markers) and a few lines added to the script, mostly calls into
`assets/scripts/lib/kit.st`. So the rule above holds: the level file gains no logic and no new kind of
entity, and what a door does stays readable and changeable where all the logic is.

**Samples and guide.** Script > Open sample writes a fresh copy of a level from `assets/samples` to the user's
settings directory, so it plays at once and the shipped one stays as it is. The guide (F7) is
`assets/guide/scripting.txt` and a reference page made from the API table. Tests compile every example in it
and compare the table with the game's own, so neither can drift.

## Not covered

- Save games. The VM state could be saved, but levels have no saves today.
- Scripted enemy AI per step, and scripted weapons. Both stay Rust.
- Movers casting baked shadows, switchable lights.
- A debugger for the language.
- Anything measured on the i3-7100U or LazyOS.
