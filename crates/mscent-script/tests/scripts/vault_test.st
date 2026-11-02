vault := mover: 'vault_door'.
assert: vault poseNow = 'closed' because: 'vault starts closed'.
(tagged: 'vault_switch') members do: [:s |
    teleport: s distance: 8.
    aimAt: s.
    fireFor: 0.8.
    wait: 0.1.
].
wait: 2.
assert: vault poseNow = 'open' because: 'vault opens once every switch is destroyed'.
shot: 'vault-open.png'.
