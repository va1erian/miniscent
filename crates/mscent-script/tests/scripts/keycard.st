card := spawn: 'red_keycard' at: (marker: 'red_key') tag: 'key'.
card onPickup: [
    player give: 'red'.
    say: 'Red keycard'.
].

autoDoor: 'red_door' travel: (x: 0 y: 0 z: 3.5) key: 'red'.
autoDoor: 'side_door' travel: (x: 0 y: 2 z: 0) key: nil.
