"The game library will own these; the tests supply them against the stub host."
define waitArrived: m [ [m moving] whileTrue: [wait: 0.05] ]
define waitDestroyed: g [ [g alive > 0] whileTrue: [wait: 0.1] ]
