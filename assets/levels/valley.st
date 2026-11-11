"The force field closes the valley until the console in the base drops it."
field := mover: 'field'.
field pose: 'off' move: (x: 0 y: 0 z: -215).
say: 'A force field closes the valley. Land on the pad by the base, B to step out'.

"The console is inside the base, through a door the ship cannot take."
console := spawn: 'terminal' at: (marker: 'console') tag: 'console'.
console onUse: [
    field moveTo: 'off' in: 3.
    say: 'Force field down. Back to the ship: B to board'.
] hold: 2.

"The end of the valley wins only in the ship."
(zone: 'finish') onEnter: [
    player onFoot
        ifTrue: [say: 'Too far to walk: fetch the ship']
        ifFalse: [win].
].
