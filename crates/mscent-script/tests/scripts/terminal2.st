kind := defineProp: 'terminal'.
kind mesh: 'terminal'.
kind radius: 1.

sentry := defineEnemy: 'sentry' base: 0.
sentry hp: 80.
sentry speed: 0.

terminal := spawn: 'terminal' at: (marker: 'sec_terminal') tag: 'term'.
terminal onUse: [
    (tagged: 'sec_drones') setFaction: 'friendly'.
    (tagged: 'sec_turrets') disable.
    say: 'Security override: drones are on your side'.
] hold: 3.
