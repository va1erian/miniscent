"Opens as the player arrives, closes after they leave. A key of nil means anyone."
define autoDoor: name travel: travel key: key [
    door := mover: name.
    door pose: 'open' move: travel.
    area := zone: name , '_area'.
    area onEnter: [
        (key isNil or: [player has: key])
            ifTrue: [door moveTo: 'open' in: 0.6]
            ifFalse: [say: 'Needs the ' , key , ' keycard'].
    ].
    area onExit: [door moveTo: 'closed' in: 0.8].
]
