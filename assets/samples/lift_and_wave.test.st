platform := mover: 'platform'.
assert: platform poseNow = 'closed' because: 'the lift starts at the bottom'.
wait: 8.
assert: platform poseNow = 'top' because: 'the lift rides up by itself'.

flyTo: (x: 16 y: 0 z: 2).
wait: 0.2.
drones := tagged: 'hall'.
assert: drones alive = 3 because: 'walking in wakes three drones'.

drones members do: [:d |
    teleport: d distance: 4.
    aimAt: d.
    fireFor: 2.
].
wait: 1.
assert: drones alive = 0 because: 'the drones can be shot down'.
assert: phase = 'won' because: 'clearing the hall wins the level'.
