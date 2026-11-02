"The platform rises 8 units, then comes down, every 5 seconds."
lift: 'platform' rise: 8 every: 5.

"Walking into the hall wakes three drones on the hall_spawn markers."
wave: 'hall' kind: 'drone'.

"A sequence is just lines of code: a handler can wait."
(zone: 'hall') onceEnter: [
    say: 'Contact!'.
    wait: 0.5.
    waitDestroyed: (tagged: 'hall').
    say: 'Sector clear'.
    win.
].
