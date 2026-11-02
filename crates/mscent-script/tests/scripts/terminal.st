terminal := spawn: 'terminal' at: (marker: 'sec_terminal') tag: 'term'.
terminal onUse: [
    (tagged: 'sec_drones') setFaction: 'friendly'.
    (tagged: 'sec_turrets') disable.
    say: 'Security override: drones are on your side'.
] hold: 3.
