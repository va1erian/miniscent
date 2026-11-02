"Shoot every switch: the first door opens."
first := mover: 'first_door'.
first pose: 'open' move: (x: 0 y: 0 z: 9).
(markers: 'door_switch') do: [:m | spawn: 'switch' at: m tag: 'door_switches'].
(tagged: 'door_switches') onAllDestroyed: [
    first moveTo: 'open' in: 0.6.
    say: 'The door is open'.
].

"The keycard opens the second door. autoDoor: is in lib/doors.st."
card := spawn: 'keycard' at: (marker: 'key_spot') tag: 'key'.
card onPickup: [player give: 'red'. say: 'Red keycard'].
autoDoor: 'red_door' travel: (x: 0 y: 0 z: 9) key: 'red'.

"Hold the use key on the terminal: the guards change sides."
terminal := spawn: 'terminal' at: (marker: 'sec_terminal') tag: 'term'.
terminal onUse: [
    (tagged: 'sec_drones') setFaction: 'friendly'.
    say: 'Security override'.
] hold: 2.

"Walking into the last room wakes two more."
(zone: 'far_room') onceEnter: [
    (markers: 'wave') do: [:m | spawn: 'drone' at: m tag: 'wave'].
    say: 'Reach the goal!'.
].
