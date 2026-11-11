# The pilot on foot

The player flies a drone ship, and can land it anywhere, step out and play on foot: a person with
the controls of a Quake-like shooter and a jetpack, who walks back to the ship and boards it to
fly on. Any level allows both. What fits where is the level's geometry, built to the sizes in the
scale guide below. `assets/levels/valley.msl` shows it: fly up a valley, land by an enemy base,
walk in through a door the ship cannot take, drop the force field that closes the valley, walk
back, board and fly on.

## Controls

| | In the ship | On foot |
|---|---|---|
| Move | WASD thrust, R/F up and down | WASD walk and strafe |
| Look | mouse, arrows; Q/E roll | mouse (no roll; the view never tips over) |
| Jump | | Space or R; held in the air, the jetpack |
| Fire | left button or Space | left button |
| Use | hold G | hold G |
| Leave / board | B, once landed | B, near the ship |

The HUD shows `B  LEAVE SHIP` when the ship is down and `B  BOARD` when the pilot is close enough,
and a JET bar beside the health while on foot. The ship and the pilot share one health bar. A
refused press (still flying, no room beside the ship, too far from it) plays the "no way" sound.

## Scale

Units are the pilot's metres in every level. The stock ship is a little over a unit across, the
size every existing level was built for. A level that wants a bigger ship says so with one line:

```text
scale -12 380 0.375  5          # x y z (where the editor draws its figures), ship size
```

The ship's collision sphere, its thrust and so its top speed, its hover bob and the chase camera
all grow with the size, so a big ship handles as the stock one does, measured in its own lengths.
The pilot never changes. No line is size 1, and every old level plays as it did, bit for bit (the
replay tests are unchanged).

The aids for building to two sizes at once:

- **`miniscent --scale-guide <size | level.msl>`** prints what each body needs, from the same
  constants the simulation runs on (`mscent_game::ScaleGuide`, `crates/mscent-game/src/scale.rs`).
  For the valley:

  ```text
  Ship: sphere radius 2.50 m, top speed 60.0 m/s, fits an opening of 5.00 m, easy 7.50 m
  Pilot: 1.80 x 0.70 m, fits a door 0.70 wide and 1.80 high, easy 1.05 x 2.20, runs 6.0 m/s,
         step 0.45 m, jump 1.00 m, jetpack 11.7 m on a full tank, slopes to 46 degrees,
         boards from 2.5 m off the hull
  A door at least 0.70 wide and 1.80 high, with its width or its height under 5.00 m, lets
  the pilot in and keeps the ship out.
  ```

- **The editor's 2D views** draw the ship's sphere at the level's size, with the pilot standing
  beside it, at the player start and at every Scale entity, so a doorway or a ceiling is judged
  against both bodies as it is drawn (`apps/editor/src/ui/paint2d_scale.rs`). A Scale entity's
  one field, Ship size, is edited in the properties panel.
- **`ScaleGuide::ship_fits` and `pilot_fits`** for a level's own tests.

## How it works

`mscent-game`:

- `pilot.rs`: the walker. Two spheres of 0.35 m stacked 1.1 m apart: the feet, which move and
  stand, and the head, which a move may not push into a ceiling or a low wall. Ground and air
  acceleration, friction and stop speed are Quake's forms; the air adds at most 0.9 m/s along a
  wish, so a jump holds its line and turning while strafing gains speed. Steps up to 0.45 m are
  climbed: the move is tried lifted and set down, and taken if it goes farther. A sphere first
  lands on a step's rounded edge, which is too steep to stand on, so the floor is looked for a
  radius further on. The jetpack lights when the jump is held in the air below 2 m/s of climb
  (so a tap is a plain jump), burns while held, and refills on the ground.
- `craft.rs`: leaving, boarding, and which body is played. The ship is landed when slower than
  1.5 m/s per size and with walkable ground no more than its radius under it. The pilot steps out
  on the ship's right, left, back or front, the first with ground, room and a clear way; the ship
  is set down and parked. Enemies, zones, pickups, the use key, shots and the goal all follow the
  played body (`Game::actor_pos`, `actor_radius`, `aim`), so a zone or a door can be for one body
  and not the other. Movers push the pilot and the parked ship both.
- `Input::board` is part of the input, so leaving and boarding replay like everything else.

`miniscent-game`: `controls.rs` maps Space to jump on foot; `session/frame.rs` puts the camera at
the pilot's eyes (no bob, no bank) and draws the crew (`ship.rs`): the parked ship on every
renderer, since it is the way back, and the played body only on the hardware renderer, in a chase
view, or as a shadow. The pilot's model is `assets/ship/pilot.msb`.

Scripts: `player onFoot` is true while the pilot is out. A test script also has `walkFor:`,
`jumpFor:` and `pressBoard`, and `teleport:`, `flyTo:` and `aimAt:` move whichever body is played.

## The valley demo

`assets/levels/valley.txt` (packed to `valley.msl`), `valley.st` and `valley.test.st`: a 1 km
valley of noise voxels with a 100 m road, the ship at size 5, a landing pad, a base with a 3 m by
2.5 m door and a guard, a console, and a force field that the console sends into the ground.
The end of the valley wins only in the ship.

```text
# the level test: field holds the ship, landing, out, through the door, the guard, the jetpack,
# the console, walking to the end does not count, boarding, the win
cargo run --release -p miniscent-game -- --level assets/levels/valley.msl --test assets/levels/valley.test.st

# the same with real controls, headless, with pictures (ends "final phase Won")
cargo run --release -p miniscent-game -- --level assets/levels/valley.msl \
    --script apps/game/scripts/valley.txt --screenshot target/shots/valley.png \
    --frames 3000 --shots-at 5,10.8,11.4,14.5,19,25,33.6,35.8,41
# add --chase --renderer hardware to see the ship and the pilot from behind

# play it
cargo run --release -p miniscent-game -- --level assets/levels/valley.msl
```

## Not done

- No crouch, swimming or ladders. Stairs are steps of up to 0.45 m.
- The pilot has the ship's gun. There is no separate weapon or animation for the pilot model
  (it is one still part).
- The ship has no gravity: landing is flying down onto the ground and pressing B. It is not
  checked that the ship is level when it lands.
- A mover pushes the pilot's feet sphere; its head is not checked against a mover's push.
- Enemies aim at the pilot's chest with one sphere of half the pilot's height, not two spheres.
- The editor has no tool to place a Scale entity; it is written in the level text (a `scale`
  line), and is then shown, moved and edited in the editor like any entity.
