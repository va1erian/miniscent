# Level scripting: choosing the language

Status: decided by the owner (November 2026). The production crate is `crates/mscent-script` (M9.4); the
prototype this page measured lived outside the repo. The design that follows from this is [scripting.md](scripting.md).

## Decision

**Write a small Smalltalk-style language in-repo instead of adding Rhai.** It came out about 17 times
smaller than Rhai in the same test program, has no dependencies, and can really suspend a script, so
the step lists an early Rhai design needed go away.

The cost is that we own a language. The prototype's core is about 1,250 lines; a production version is
estimated at 2,000 lines plus tests, one work item, and smaller than the mover work.

```smalltalk
vault := mover: 'vault_door'.
vault pose: 'open' move: (x: 0 y: 0 z: 4).

(markers: 'vault_switch') do: [:m | spawn: 'switch' at: m tag: 'vault_switch'].

(tagged: 'vault_switch') onAllDestroyed: [
    vault moveTo: 'open' in: 1.5.
    say: 'Vault unlocked'.
].

(zone: 'hall') onceEnter: [
    entry moveTo: 'closed' in: 0.5.
    wait: 1.
    spawnAll: 'drone' at: (markers: 'hall_wave1') tag: 'wave1'.
    waitDestroyed: (tagged: 'wave1').
    player hp < 50 ifTrue: [player heal: 50].
    wayOut moveTo: 'open' in: 1.5.
].
```

`wait:` pauses only that handler. Library code is plain script too:

```smalltalk
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

## Size

The whole game, renderer and desktop backend included, is 1.22 MB in today's release profile, or
0.93 MB optimized for size. Rhai at 3.88 MB would have more than quadrupled it.

Bytes of Windows binary, dev machine. "Added" subtracts an empty program from the release figure.

| Option | Release | Added | LTO, `opt-level=s`, stripped | Also `opt-level=z`, `panic=abort` |
|---|---|---|---|---|
| Empty program | 131,072 | | 112,640 | 109,056 |
| **Smalltalk-style prototype** | 359,424 | **228 KB** | 290,304 | 239,104 |
| rust_lisp 0.18 | 398,848 | 268 KB | 300,032 | 223,744 |
| Lua 5.4 through mlua 0.11 | 548,864 | 418 KB | 400,896 | 397,312 |
| piccolo 0.3 | 748,544 | 617 KB | 551,936 | 412,672 |
| Koto 0.16 | 2,240,512 | 2.1 MB | 1,529,344 | 1,015,296 |
| Rhai 1.27, `new_raw`, trimmed features | 2,962,432 | 2.8 MB | 2,037,248 | 1,056,256 |
| Rhai 1.27, the design's stub API | 4,105,728 | 4.0 MB | 2,808,832 | 1,378,816 |

These are not exact like for like. The prototype and the Rhai stub API each bind about 30 game
functions, and the prototype also carries its test harness, so its figure is high. The other rows
bind one or two functions, so theirs are low; a real API would add to them. The trimmed Rhai row uses
a much smaller program, so its gap to the stub row is not all features. Rune and luars were not
measured.

The last column is not available to the game: `opt-level=z` would slow the renderer, unless limited
per package, and `panic=abort` would remove the panic catch Instant Play relies on.

## Why not the others

| Option | Against |
|---|---|
| **Lua 5.4 through mlua** | The real runner-up: small, coroutines, agents know it well. But it brings a C build and FFI into a workspace that has neither in its core crates, undefined names fail only when reached, and its state cannot be cloned |
| piccolo | Its README calls it very experimental, most of the stdlib is missing, and the last release was June 2024 |
| rust_lisp | Last release December 2022, and its API hands out `Rc`, so it cannot move to the Instant Play thread. Suspension and limits were not checked |
| Koto | Over 2 MB and its only limit is wall-clock, which breaks replay |
| Rhai | Size, no suspension |
| Embedding LazyVM | The VM crate alone is 405 KB of source, its budget is 4 MB, and Java needs a compile step, which defeats the agent loop |

The LazyOS port does not rule out C. LazyVM's `docs/compiler.md` says LazyOS runs unmodified
`x86_64-unknown-linux-musl` programs, so a C dependency is probably buildable there, though I did not
test it. C is still a real cost here, but it is not a blocker.

## What the prototype does

Lexer and parser 372 lines, compiler and checks 318, VM 566, plus a 276-line test harness. No
dependencies, `unsafe_code = "forbid"`. It ran all of the design's use cases against a stub host that
logs commands, so nothing here is proven against the real game.

| What | Prototype | Rhai |
|---|---|---|
| Fire an event into a closure | 0.17 microseconds | 0.65 |
| Compile 600 lines | 0.61 ms | 0.67 ms |
| Run its top level | 0.012 ms | 0.11 ms |
| Worst-case script time per step | about 0.6 ms | about 5 ms |

**Real suspension.** Frames live in a `Vec`, not on the Rust stack, so a `wait:` just parks the task.
The stub log showed a 1 second wait resuming at tick 120 of the 120 Hz step, and crusher cycles at
119, 239 and 359. `waitUntil:`, `every:do:` and `after:do:` are one-line script functions.

**Typos fail at load, even in a handler that never fires.** Selectors form a closed set and level
names are string literals, so both are checked at compile time. Rhai cannot do this.

```
line 1: nothing understands 'moveToo:in:' (did you mean 'moveTo:in:'?)
line 1: mover: 'vault_dor': the level has no such name (did you mean 'vault_door'?)
```

**State is plain data.** Everything is indexed `Vec`s, so the VM can derive `Clone` and could be
saved. That is why `Game` can keep its `Clone`, and it makes save games possible later.

**Hostile input.** Infinite loop, string bomb, recursion, array bomb, 100,000 nested parentheses and
100,000 nested blocks all gave errors. 30,000 seeded byte mutations of real scripts (1,920 still
loaded, 137 runtime errors) finished without a panic.

## Can an agent write it?

This was the main worry, since no model has seen this language. Two fresh agents on the small Haiku
model got the same five new scenarios and a cheat sheet, one for this language and one for Rhai. They
could not run anything or revise, and the check fired every handler against a stub.

| Script | Smalltalk-style lines | Rhai lines | Result |
|---|---|---|---|
| Timed door | 17 | 18 | Both correct |
| Key and turrets | 21 | 17 | Both correct |
| Three waves | 17 | 32 | Rhai invented `hall_wave2`, `hall_wave3` and tag `wave3`, which the level does not have |
| Countdown | 30 | 29 | Both correct |
| Lift | 23 | 24 | Both correct, but Rhai needed a recursive function building nested step lists |

Both languages loaded and ran 5 of 5 with no errors. Correctness is my reading of the scripts, and the
Rhai stub does not check names, so it did not catch the invented ones; a real run would have reported
them. This is one run of one small sample, enough to say the language is not a barrier, not that it
beats Rhai.

## Known weaknesses

All were reproduced in the prototype. The first group is fixed in M9.4, each with a test that fails without the fix.

| Weakness | Fix |
|---|---|
| A typo in an assignment declares a new variable: `timerRuning := true` loads without complaint | Fixed: the load result carries a warning for a variable that is assigned and never read, for `--check` to print |
| `fork:` in a loop left 2,857 tasks alive; only the op budget stopped it | Fixed: 128 live tasks, an error at the cap |
| Up to 200,000 objects of up to 4 KB could reach about 800 MB | Fixed: a 16 MiB heap counted in bytes with a per-object overhead |
| A missing period gives `nothing understands 'say:say:'` | Fixed: if the unknown selector splits into known messages, the error names the missing `.` and its line |
| Errors say `level:1` or `f::1`, not always file and line | Fixed: every error, at load and at run time, gives its source file and line |

These are the language's character, and are documented rather than fixed:

- `2 + 3 * 4` is 20, because binary operators run left to right, as in Smalltalk. Keeping this avoids
  a precedence table; `--check` could warn on mixed operators without parentheses.
- It is unfamiliar to read at first, for people who do not know Smalltalk.
- There are no classes, debugger or editor support. Syntax colouring would be new work.

## Taken from LazyVM

[LazyVM](https://github.com/va1erian/lazyvm) reached many of the same answers, which is reassuring,
and it has a few the prototype lacks:

| From LazyVM | Status here |
|---|---|
| Slot table with checked indices, so no `unsafe`; precise non-moving mark-sweep | Already the same |
| Frames in a `Vec`, not the Rust stack | Already the same |
| Collect only at safe points, never during allocation | Already the same, at step boundaries |
| Natives as `fn(&mut Vm, &[Value])`, arguments rooted by staying on the stack | Already the same, with the host passed in, so no `lend` and no `unsafe` |
| References carry a generation, so a stale one is detected instead of aliasing | Done (M9.4) |
| Heap limit counted in bytes, with a per-object overhead; a separate budget for frame slots | Done for the heap. Stacks sit outside it, bounded by 128 tasks times 4,096 values, about 6 MiB |
| `cargo fuzz` target with a stored corpus | Done: `crates/mscent-script/fuzz`, seeded from the example scripts, and the seeded loop stays as an ordinary test. No corpus is stored. On Windows it needs the Visual Studio ASAN DLL on `PATH` |
| **A size budget that fails CI** (`tools/size.py`, decision D14) | Done for the whole game (M9.1) |

## Effort

The prototype took one session. The language item in [work-items.md](work-items.md) (M9.4) is roughly
twice its size: about 2,000 lines of code and 1,000 of tests, which I estimate at one or two agent
sessions. API binding, tracing and tests are needed with any language, so the real extra cost over Rhai
is the language core and its tests.

M9.4 came to 3,100 lines of code and 2,200 of tests, against the 2,000 and 1,000 estimated above. Most of the difference is checks the prototype did not have. Measured under the workspace release profile (`opt-level = "s"`, fat LTO), the crate adds 172,032 bytes to a program that loads and runs a script with 30 natives, against the 228 KB the prototype showed in the plain release column.

The lasting cost is ownership, and the guard against it is a rule: the language gets no classes, modules
or new syntax unless a level cannot be written without them.
