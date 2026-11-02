# What movers cost a frame

`cargo run --release -p mscent-render --example mover_bench`: the showcase from a
fixed view with box doors in sight, 640x360, one thread, High quality. Counts take
turns batch by batch, and the fastest batch of each is reported. Run in order, the
same machine's drifting clock made four movers read anywhere from 7.7 to 12.4 ms.

Three runs on the dev machine, not the i3-7100U:

| Movers | ms per frame |
|---|---|
| 0 | 7.56 to 7.58 |
| 1 | 7.55 to 7.58 |
| 4 | 7.62 to 7.71 |
| 16 | 7.78 to 7.80 |

So about 0.014 ms a mover for small doors. Movers draw before the world, so a door
that covers the view can save shading rather than add it. That was not measured.
