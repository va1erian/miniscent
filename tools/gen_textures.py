#!/usr/bin/env python
"""Procedural, seamlessly tiling 128x128 textures for miniscent.

Writes, per texture, into assets/textures/:
  <name>.png         albedo (RGB)
  <name>_h.png       height map (8-bit grey)
  <name>_n.png       tangent-space normal map (RGB, +Y up in texture space)
"""
import os
import numpy as np
from PIL import Image

N = 128
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
rng = np.random.default_rng(1996)


def tile_noise(octaves=5, base=4, persistence=0.55):
    """Periodic fractal value noise in [0,1]."""
    acc = np.zeros((N, N))
    amp, total = 1.0, 0.0
    for o in range(octaves):
        g = base * (2 ** o)
        if g >= N:
            break
        grid = rng.random((g, g))
        # bicubic-ish smooth upsample with wrap via FFT zero-padding
        f = np.fft.fft2(grid)
        f = np.fft.fftshift(f)
        pad = np.zeros((N, N), dtype=complex)
        lo = (N - g) // 2
        pad[lo:lo + g, lo:lo + g] = f
        up = np.real(np.fft.ifft2(np.fft.ifftshift(pad))) * (N * N) / (g * g)
        up = (up - up.min()) / (np.ptp(up) + 1e-9)
        acc += up * amp
        total += amp
        amp *= persistence
    acc /= total
    return (acc - acc.min()) / (np.ptp(acc) + 1e-9)


def to_normal(h, strength):
    """Height [0,1] -> tangent-space normal, wrapped differences (tiles cleanly)."""
    dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5 * strength
    dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5 * strength
    n = np.dstack([-dx, dy, np.ones_like(h)])
    n /= np.linalg.norm(n, axis=2, keepdims=True)
    return n


def save(name, albedo, height, strength):
    os.makedirs(OUT, exist_ok=True)
    a = np.clip(albedo, 0, 1)
    n = to_normal(height, strength)
    Image.fromarray((a * 255 + 0.5).astype(np.uint8)).save(f"{OUT}/{name}.png")
    Image.fromarray((np.clip(height, 0, 1) * 255 + 0.5).astype(np.uint8), "L").save(f"{OUT}/{name}_h.png")
    Image.fromarray(((n * 0.5 + 0.5) * 255 + 0.5).astype(np.uint8)).save(f"{OUT}/{name}_n.png")
    print("wrote", name)


def brick():
    rows, bw = 8, 32  # 8 rows of 16px, bricks 32px wide -> 4 per row
    rh = N // rows
    y, x = np.mgrid[0:N, 0:N]
    row = y // rh
    xs = (x + (row % 2) * (bw // 2)) % N
    cell = xs // bw
    fx, fy = xs % bw, y % rh
    mortar = (fx < 2) | (fy < 2)
    # per-brick tint
    tint = rng.random((rows, N // bw + 1))
    t = tint[row, cell]
    base = np.dstack([0.55 + 0.25 * t, 0.22 + 0.10 * t, 0.15 + 0.06 * t])
    grain = tile_noise(5, 8)
    base *= (0.75 + 0.5 * grain)[..., None]
    mort = np.array([0.45, 0.43, 0.40]) * np.ones((N, N, 1)) * (0.8 + 0.4 * tile_noise(4, 16))[..., None]
    alb = np.where(mortar[..., None], mort, base)
    h = np.where(mortar, 0.0, 0.7 + 0.3 * grain)
    # soften brick edges a little
    for _ in range(1):
        h = (h + np.roll(h, 1, 0) + np.roll(h, -1, 0) + np.roll(h, 1, 1) + np.roll(h, -1, 1)) / 5
    return alb, h, 5.0


def metal():
    y, x = np.mgrid[0:N, 0:N]
    panel = 64
    fx, fy = x % panel, y % panel
    seam = (fx < 2) | (fy < 2) | (fx >= panel - 1) | (fy >= panel - 1)
    scratch = tile_noise(6, 8, 0.65)
    brushed = np.tile(tile_noise(4, 2)[:, :1], (1, N)) * 0.5 + 0.5 * scratch
    base = 0.38 + 0.22 * brushed
    alb = np.dstack([base * 0.92, base * 0.97, base * 1.05])
    # rivets
    h = np.full((N, N), 0.5) + 0.04 * (brushed - 0.5)
    for cx in (8, 56, 72, 120):
        for cy in (8, 56, 72, 120):
            d = np.hypot(((x - cx + N / 2) % N) - N / 2, ((y - cy + N / 2) % N) - N / 2)
            h += np.clip(1 - d / 3.5, 0, 1) * 0.35
            alb[d < 3.5] *= 1.25
    h = np.where(seam, 0.1, h)
    alb[seam] *= 0.45
    return alb, np.clip(h, 0, 1), 4.0


def rock():
    n1 = tile_noise(6, 4, 0.6)
    n2 = tile_noise(4, 16, 0.5)
    h = np.clip(n1 * 0.85 + 0.15 * n2, 0, 1)
    # ridged detail
    ridge = 1 - np.abs(2 * tile_noise(5, 4, 0.55) - 1)
    h = np.clip(0.6 * h + 0.4 * ridge, 0, 1)
    c = 0.30 + 0.35 * h
    alb = np.dstack([c * 1.0, c * 0.94, c * 0.86])
    return alb, h, 7.0


def floor_tile():
    y, x = np.mgrid[0:N, 0:N]
    t = 32
    fx, fy = x % t, y % t
    grout = (fx < 2) | (fy < 2)
    chk = (((x // t) + (y // t)) % 2).astype(float)
    wear = tile_noise(5, 8)
    base = 0.30 + 0.10 * chk + 0.12 * wear
    alb = np.dstack([base * 0.9, base, base * 1.05])
    # hazard chevron stripe on the middle row of tiles
    # Supersampled: a hard diagonal on the texel grid is a staircase that
    # magnified bilinear sampling turns into a visible zigzag.
    ss = 8
    sy, sx = np.mgrid[0:N * ss, 0:N * ss]
    hard = ((((sx + sy) // 8 // ss) % 2) == 0).astype(float)
    stripe = hard.reshape(N, ss, N, ss).mean(axis=(1, 3))
    band = (y >= 48) & (y < 80)
    hz = np.dstack([0.85 * np.ones_like(base), 0.65 * np.ones_like(base), 0.05 * np.ones_like(base)]) * (0.7 + 0.3 * wear)[..., None]
    dark = np.array([0.08, 0.08, 0.08]) * np.ones((N, N, 1))
    mix = stripe[..., None]
    alb = np.where(band[..., None], hz * mix + dark * (1 - mix), alb)
    alb[grout] *= 0.35
    h = np.where(grout, 0.0, 0.8 + 0.1 * wear)
    return alb, h, 4.0


def lightpanel():
    y, x = np.mgrid[0:N, 0:N]
    d = np.maximum(np.abs(x - N / 2), np.abs(y - N / 2))
    frame = d > 52
    inner = d < 46
    grid = ((x % 16) < 1) | ((y % 16) < 1)
    glow = np.clip(1.0 - np.hypot(x - N / 2, y - N / 2) / 90, 0.55, 1)
    alb = np.dstack([glow * 1.0, glow * 0.95, glow * 0.75])
    alb[~inner & ~frame] = 0.2
    alb[frame] = np.array([0.22, 0.24, 0.28]) * (0.8 + 0.4 * tile_noise(4, 8))[frame][:, None]
    alb[inner & grid] *= 0.7
    h = np.where(frame, 0.8, np.where(inner, 0.45 - 0.1 * grid, 0.2))
    return alb, h, 3.0


def worley(points, seed):
    """Periodic distance (in texels) to the nearest of `points` jittered sites."""
    r = np.random.default_rng(seed)
    sites = r.random((points, 2)) * N
    y, x = np.mgrid[0:N, 0:N]
    near = np.full((N, N), 1e9)
    second = np.full((N, N), 1e9)
    for sx, sy in sites:
        dx = np.minimum(np.abs(x - sx), N - np.abs(x - sx))
        dy = np.minimum(np.abs(y - sy), N - np.abs(y - sy))
        d = np.hypot(dx, dy)
        second = np.where(d < near, near, np.minimum(second, d))
        near = np.minimum(near, d)
    return near, second


def smooth_step(lo, hi, x):
    t = np.clip((x - lo) / (hi - lo), 0, 1)
    return t * t * (3 - 2 * t)


def ramp(t, a, b):
    t = t[..., None]
    return np.array(a) * (1 - t) + np.array(b) * t


def cap():
    """A toadstool's skin: red with cream warts, big enough to read at a distance."""
    near, _ = worley(110, 11)
    wart = 1 - smooth_step(1.5, 2.8, near)
    mottle = tile_noise(4, 4)
    skin = ramp(mottle, (0.50, 0.08, 0.05), (0.80, 0.20, 0.09))
    a = skin * (1 - wart[..., None]) + np.array((0.93, 0.87, 0.70)) * wart[..., None]
    return a, 0.35 + 0.1 * mottle + 0.5 * wart, 3.0


def stem():
    """Pale flesh with fine fibres running up it (V is up on a wall)."""
    fibres = tile_noise(4, 8, 0.5)
    streak = np.fft.ifft(np.fft.fft(fibres, axis=0) * (np.abs(np.fft.fftfreq(N)) < 0.03), axis=0).real
    streak = (streak - streak.min()) / (np.ptp(streak) + 1e-9)
    blotch = tile_noise(3, 4)
    a = ramp(streak, (0.62, 0.55, 0.42), (0.90, 0.84, 0.70)) * (0.88 + 0.12 * blotch[..., None])
    return a, 0.3 + 0.6 * streak, 2.0


def bark():
    """Dark furrows running up the trunk, with ridges broken into plates."""
    grain = tile_noise(5, 8, 0.6)
    ridge = np.fft.ifft(np.fft.fft(grain, axis=0) * (np.abs(np.fft.fftfreq(N)) < 0.05), axis=0).real
    ridge = (ridge - ridge.min()) / (np.ptp(ridge) + 1e-9)
    furrow = smooth_step(0.38, 0.62, ridge)
    near, second = worley(30, 13)
    plate = smooth_step(0.0, 3.0, second - near)
    h = furrow * (0.6 + 0.4 * plate)
    a = ramp(h, (0.16, 0.10, 0.06), (0.66, 0.47, 0.29)) * (0.8 + 0.3 * tile_noise(4, 8)[..., None])
    return a, h, 5.0


def coral():
    """Rose stone pitted with polyp cups in a close, even field."""
    near, second = worley(220, 17)
    cup = 1 - smooth_step(0.6, 2.2, near)
    rim = smooth_step(0.0, 2.0, second - near)
    mottle = tile_noise(4, 4)
    body = ramp(mottle, (0.76, 0.34, 0.30), (0.96, 0.62, 0.50))
    a = body * (1 - 0.5 * cup[..., None]) * (0.85 + 0.15 * rim[..., None])
    return a, 0.75 - 0.6 * cup + 0.1 * mottle, 4.0


def wax():
    """Honey-coloured wax: warm, smooth, with soft pooled runs."""
    flow = tile_noise(4, 4, 0.45)
    run = np.fft.ifft(np.fft.fft(tile_noise(3, 8), axis=0) * (np.abs(np.fft.fftfreq(N)) < 0.04), axis=0).real
    run = (run - run.min()) / (np.ptp(run) + 1e-9)
    t = 0.6 * flow + 0.4 * run
    a = ramp(t, (0.62, 0.36, 0.07), (0.98, 0.74, 0.25))
    return a, 0.4 + 0.3 * t, 1.5


if __name__ == "__main__":
    for name, fn in [("brick", brick), ("metal", metal), ("rock", rock),
                     ("floor", floor_tile), ("lightpanel", lightpanel)]:
        a, h, s = fn()
        save(name, a, h, s)
    for name, fn in [("bark", bark), ("cap", cap), ("coral", coral),
                     ("stem", stem), ("wax", wax)]:
        a, h, s = fn()
        save(name, a, h, s)
