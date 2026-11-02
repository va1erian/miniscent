gate := mover: 'gate'.
assert: gate poseNow = 'closed' because: 'the gate starts shut'.
flyTo: (x: 1 y: 0 z: 0).
wait: 1.5.
assert: gate poseNow = 'open' because: 'the gate opens as the player arrives'.
flyTo: (x: -8 y: 0 z: 0).
wait: 1.5.
assert: gate poseNow = 'closed' because: 'the gate shuts after the player leaves'.
flyTo: (x: 17 y: 0 z: 0).
wait: 0.2.
assert: phase = 'won' because: 'the way out wins the level'.
