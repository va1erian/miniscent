first := mover: 'first_door'.
red := mover: 'red_door'.
assert: first poseNow = 'closed' because: 'the first door starts closed'.

(tagged: 'door_switches') members do: [:s |
    teleport: s distance: 3.
    aimAt: s.
    fireFor: 0.7.
].
wait: 1.
assert: first poseNow = 'open' because: 'shooting every switch opens the door'.

teleport: (marker: 'sec_terminal') distance: 2.
aimAt: (marker: 'sec_terminal').
useFor: 2.5.
wait: 0.1.
hurt := player hp.
wait: 2.
assert: player hp = hurt because: 'the override makes the guards friendly'.

flyTo: (x: 14 y: 0 z: 0).
wait: 0.5.
assert: red poseNow = 'closed' because: 'no keycard, no door'.

flyTo: (marker: 'key_spot').
wait: 0.1.
assert: (player has: 'red') because: 'touching the keycard picks it up'.

flyTo: (x: 14 y: 0 z: 0).
wait: 1.
assert: red poseNow = 'open' because: 'the keycard opens the door'.

flyTo: (x: 24 y: 0 z: 0).
wait: 0.1.
assert: (tagged: 'wave') alive = 2 because: 'entering the last room spawns the wave'.

flyTo: (x: 32 y: 0 z: 0).
wait: 0.2.
assert: phase = 'won' because: 'reaching the goal wins'.
