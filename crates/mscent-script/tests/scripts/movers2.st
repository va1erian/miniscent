trap := mover: 'trapdoor'.
trap pose: 'open' turn: (x: 1 y: 0 z: 0) degrees: 90 about: (marker: 'trap_hinge').
(zone: 'trap_plate') onEnter: [trap moveTo: 'open' in: 0.4].

lift := mover: 'lift'.
lift pose: 'top' move: (x: 0 y: 0 z: 12).
lift whenBlocked: 'reverse'.
every: 6 do: [
    lift moveTo: (lift poseNow = 'top' ifTrue: ['closed'] ifFalse: ['top']) in: 2.5.
].

crusher := mover: 'crusher'.
crusher pose: 'down' move: (x: 0 y: 0 z: -3).
crusher whenBlocked: 'crush' damage: 40.
fork: [
    [true] whileTrue: [
        crusher moveTo: 'down' in: 0.5.
        waitArrived: crusher.
        wait: 1.
        crusher moveTo: 'closed' in: 2.
        waitArrived: crusher.
        wait: 1.
    ].
].
