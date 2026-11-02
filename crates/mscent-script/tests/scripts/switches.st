vault := mover: 'vault_door'.
vault pose: 'open' move: (x: 0 y: 0 z: 4).

(markers: 'vault_switch') do: [:m | spawn: 'switch' at: m tag: 'vault_switch'].

switches := tagged: 'vault_switch'.
switches onEachDestroyed: [say: switches alive , ' left'].
switches onAllDestroyed: [
    vault moveTo: 'open' in: 1.5.
    say: 'Vault unlocked'.
].
