vault := mover: 'vault'.
red := mover: 'red_door'.
assert: vault poseNow = 'closed' because: 'the vault starts shut'.

(tagged: 'vault_switches') members do: [:s |
    teleport: s distance: 3.
    aimAt: s.
    fireFor: 0.7.
].
wait: 1.
assert: vault poseNow = 'open' because: 'shooting both switches opens the vault'.

teleport: (marker: 'console') distance: 2.
aimAt: (marker: 'console').
useFor: 2.5.
flyTo: (marker: 'medkit').
wait: 0.1.

flyTo: (x: 14 y: 0 z: 0).
wait: 0.5.
assert: red poseNow = 'closed' because: 'no keycard, no door'.

flyTo: (marker: 'vault_key').
wait: 0.1.
assert: (player has: 'red') because: 'touching the keycard gives the key'.

flyTo: (x: 14 y: 0 z: 0).
wait: 1.
assert: red poseNow = 'open' because: 'the keycard opens the door'.

flyTo: (x: 30 y: 0 z: 0).
wait: 0.2.
assert: phase = 'won' because: 'the finish wins the level'.
