# Audio

The game plays sound effects. They are Freedoom's, and sound is presentation
only: like the hardware renderer's look, it never feeds back into play.

## What plays

`apps/game/src/sound.rs` turns each game event (`mscent_game::Event`) into at
most one sound, `Cue`. Events that name something in the world (an enemy's
shot, a prop being picked up) are positional: quieter with distance (half
volume at 16 units) and panned by which side of the player they are on. The
rest (the player's own shot, being hurt, a door starting) play centred.

| Event | Cue | File |
| --- | --- | --- |
| `PlayerFired` | player's shot | `dspistol` |
| `EnemyFired` | enemy's shot | `dsfirsht` |
| `EnemyHit` (still alive) | enemy hurt | `dspopain` |
| `EnemyKilled` | enemy dies | `dspodth1` |
| `PlayerHurt` / `PlayerDied` | hurt / death | `dsplpain` / `dspldeth` |
| `GoalReached`, `ScriptWon` | win | `dsgetpow` |
| `ScriptLost`, `MoverBlocked` | no way | `dsnoway` |
| `Restarted` | restart | `dspstart` |
| `MoverSent` / `MoverHit` | mover starts / struck | `dsstnmov` / `dsmetal` |
| `PropPicked` / `PropUsed` | pick up / use | `dsitemup` / `dsswtchn` |
| `PropHit` / `PropDestroyed` | struck / destroyed | `dstink` / `dsbarexp` |
| `EnemySpawned` | spawn | `dstelept` |

A killing blow reports a hit and a kill; only the death sounds.

## Where the files are, and their license

`assets/sounds/*.wav`, from Freedoom 0.13.0, unmodified. Freedoom is under the
modified BSD 3-clause license: use, modification and redistribution are free,
commercial or not, if the copyright notice and conditions travel with it
(`assets/sounds/COPYING-FREEDOOM.txt`) and Freedoom's name is not used to
endorse the game. Its contribution rules require every asset to be free of
extra restrictions, and forbid anything taken from Doom. The provenance, the
archive checksum and the file-by-file use are in
[assets/sounds/README.md](../assets/sounds/README.md). A game build that
ships a binary must ship that folder, or at least its license file.

Replace any sound by putting a `.wav` of the same name in the folder. The
reader takes 8 and 16 bit PCM, mono or stereo, 4 to 96 kHz, up to 2M frames
(`mscent-audio/src/wav.rs`); a file it cannot read is reported on stderr and
skipped.

## How it is built

* `crates/mscent-audio`: the `.wav` reader, a `SoundBank` (a folder by file
  stem), and a `Mixer` (24 voices, linear resampling to the device's rate,
  constant-power pan; the oldest voice is replaced past the cap). It depends on
  nothing and plays nothing. Feature `device` adds `device::Output`, which opens
  the default output with [cpal](https://crates.io/crates/cpal) (Apache-2.0),
  the crate's only dependency, and mixes in the device's callback. The game
  thread sends requests over a bounded channel and never waits on the device; a
  full queue drops the request.
* `miniscent-game`'s `audio` feature (default) turns `device` on. The software-only
  build (`--no-default-features --features desktop`, LazyOS's) has no sound and
  the size budget it had. `GameSession::set_sounds` attaches a `Sounds`;
  `main.rs` does it for a window only, so headless runs, tests and screenshots
  are silent, and `--mute` turns it off in a window.
* No device, an unsupported sample format or missing files print one line and
  the game carries on silent.

## Tests

`crates/mscent-audio`: the reader (8 and 16 bit, stereo averaged, unknown
chunks, a size past the end, every truncation of a good file, hostile headers,
the length cap), the mixer (equal power, hard pan, voices freed, resampling,
NaN and out-of-range gain and pan, the voice cap, no clipping), and every
shipped file parses, is audible and is listed beside the license.
`apps/game/tests/sound.rs`: the scripted corridor win makes the player's shot,
exactly one enemy death and one win; the same run with and without sound ends
in the same phase, tick and position; an enemy dead ahead is centred and
quiet, and the sides flip as the player turns; a kill does not also sound a
pain; events naming things that do not exist are silent.

Checked by running: `cargo run --release -p miniscent-game -- --autoclose-ms 2500`
opens the default output on the RTX 5070 Ti test machine with no `no sound:`
line and exits 0. Not verified: that it can be heard. The tests read the mixed
samples and the requests, not a speaker, and nobody has listened to the mix.

The default build grew by 138,240 bytes (cpal and the mixer), so its size budget
is 6.5 MB (`tools/check_size.py`); the software-only build has no cpal.
