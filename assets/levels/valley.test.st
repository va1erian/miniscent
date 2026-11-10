field := mover: 'field'.
assert: field poseNow = 'closed' because: 'the field starts up'.
assert: player onFoot not because: 'the level starts in the ship'.

"The field holds the ship back."
flyTo: (x: 0 y: 590 z: 30).
aimAt: (x: 0 y: 700 z: 30).
wait: 2.
assert: ((player pos) at: 2) < 598 because: 'the field stops the ship'.

"Set down on the pad and step out."
flyTo: (x: -16 y: 420 z: 4).
wait: 0.2.
pressBoard.
assert: player onFoot because: 'the pilot steps out of the landed ship'.
wait: 1.

"Walk in through the base door, which the ship could never take."
flyTo: (x: 4 y: 419.25 z: 1).
wait: 1.
aimAt: (x: 30 y: 419.25 z: 1.3).
walkFor: 2.5.
assert: ((player pos) at: 1) > 14 because: 'the pilot walks in through the door'.

"The guards: shoot them on foot."
(tagged: 'guards') members do: [:g |
    teleport: g distance: 5.
    wait: 0.5.
    aimAt: g.
    fireFor: 1.5.
].
assert: (tagged: 'guards') alive = 0 because: 'the pilot shoots the guards'.

"Jump and jetpack: up off the floor, then back down."
jumpFor: 0.8.
assert: ((player pos) at: 3) > 2 because: 'the jetpack lifts the pilot'.
wait: 2.

"Hold the use key on the console."
teleport: (marker: 'console') distance: 2.
wait: 1.
aimAt: (marker: 'console').
useFor: 2.5.
wait: 3.5.
assert: field poseNow = 'off' because: 'the console drops the field'.

"Walking to the end does not count."
flyTo: (x: 0 y: 930 z: 2).
wait: 0.5.
assert: phase = 'playing' because: 'the level is won in the ship'.

"Back to the ship, board, and fly on to the end."
flyTo: (x: -12 y: 420 z: 1.5).
wait: 1.
pressBoard.
assert: player onFoot not because: 'the pilot boards beside the ship'.
flyTo: (x: 0 y: 930 z: 30).
wait: 0.2.
assert: phase = 'won' because: 'reaching the end in the ship wins'.
