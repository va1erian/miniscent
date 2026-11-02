"The functions behind the editor's Script > Add entity menu. It is ordinary
script, so read it, copy a function into a level's script and change it there
under a new name, or add to it here."

"A door that opens once every switch is shot. The switches stand on the markers
whose names start with the door's name and _switch."
define switchDoor: name travel: travel [
    door := mover: name.
    door pose: 'open' move: travel.
    switches := name , '_switches'.
    (markers: name , '_switch') do: [:m | spawn: 'switch' at: m tag: switches].
    (tagged: switches) onAllDestroyed: [door moveTo: 'open' in: 0.6].
]

"A keycard on a marker. Touching it gives the player the key of that name, which
autoDoor:travel:key: asks for."
define keycardAt: spot gives: key [
    card := spawn: 'keycard' at: (marker: spot) tag: spot.
    card onPickup: [
        player give: key.
        say: 'Picked up ' , key.
    ].
]

"A platform that rides up and down by itself. It reverses if something blocks it."
define lift: name rise: height every: seconds [
    platform := mover: name.
    platform pose: 'top' move: (x: 0 y: 0 z: height).
    every: seconds do: [
        platform moveTo: (platform poseNow = 'top' ifTrue: ['closed'] ifFalse: ['top']) in: seconds / 2.
    ].
]

"Enemies appear on the markers whose names start with the zone's name and _spawn,
the first time the player walks into the zone."
define wave: name kind: kind [
    (zone: name) onceEnter: [
        spawnAll: kind at: (markers: name , '_spawn') tag: name.
    ].
]

"A terminal on a marker: holding the use key for that long runs the block."
define terminal: spot hold: seconds do: action [
    console := spawn: 'terminal' at: (marker: spot) tag: spot.
    console onUse: [action value] hold: seconds.
]

"A pickup on a marker that repairs the player."
define healthPack: spot heals: amount [
    pack := spawn: 'crate' at: (marker: spot) tag: spot.
    pack onPickup: [
        player heal: amount.
        say: 'Repaired'.
    ].
]

"Reaching this zone wins the level."
define exitZone: name [
    (zone: name) onceEnter: [win].
]
