"Shoot both switches: switchDoor: finds them by name, vault_switch_1 and _2."
switchDoor: 'vault' travel: (x: 0 y: 0 z: 8).

"The keycard gives the key 'red', and this door asks for it."
keycardAt: 'vault_key' gives: 'red'.
autoDoor: 'red_door' travel: (x: 0 y: 0 z: 8) key: 'red'.

"Hold G on the terminal. What it does is yours to write: this one repairs."
terminal: 'console' hold: 2 do: [
    player heal: 25.
    say: 'Hull patched from the console'.
].

"Or just pick this up."
healthPack: 'medkit' heals: 50.

exitZone: 'finish'.
