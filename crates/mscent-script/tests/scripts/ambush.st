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
