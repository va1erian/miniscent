"Your first script. Press F5 to play it, Esc to come back.

The level holds a brush named 'gate', a zone named 'gate_area' and a zone named
'way_out'. Nothing in the level says what they do: these lines do."

"Opens as the player comes near. Try another travel, or a key: 'red'."
autoDoor: 'gate' travel: (x: 0 y: 0 z: 8) key: nil.

"Say something once, on the way through."
(zone: 'gate_area') onceEnter: [say: 'Welcome. The way out is ahead.'].

"exitZone: is in the kit library: reaching this zone wins the level."
exitZone: 'way_out'.
