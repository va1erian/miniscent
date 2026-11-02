#!/usr/bin/env python
"""Procedural, seamlessly tiling 128x128 textures for miniscent.

Writes, per texture, into assets/textures/:
  <name>.png         albedo (RGB)
  <name>_h.png       height map (8-bit grey)
  <name>_n.png       tangent-space normal map (RGB, +Y up in texture space)

The sky textures at the end are bigger than N, some have an alpha channel, and
they draw from their own generator so the shared stream above is untouched.
"""
import os
import numpy as np
from PIL import Image

N = 128
OUT = os.path.join(os.path.dirname(__file__), "..", "assets", "textures")
rng = np.random.default_rng(1996)
sky_rng = np.random.default_rng(2112)


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


def save(name, albedo, height, strength, alpha=None):
    """Write the maps, of whatever size the arrays are. `alpha` (0..1, shaped
    like the height) makes the albedo RGBA."""
    os.makedirs(OUT, exist_ok=True)
    a = np.clip(albedo, 0, 1)
    n = to_normal(height, strength)
    if alpha is not None:
        a = np.dstack([a, np.clip(alpha, 0, 1)])
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


def streaks(noise, cut):
    """Noise low-passed up the image, so it runs along V as fibres and runs do."""
    keep = (np.abs(np.fft.fftfreq(N)) < cut)[:, None]
    s = np.fft.ifft(np.fft.fft(noise, axis=0) * keep, axis=0).real
    return (s - s.min()) / (np.ptp(s) + 1e-9)


def smooth_step(lo, hi, x):
    t = np.clip((x - lo) / (hi - lo), 0, 1)
    return t * t * (3 - 2 * t)


def ramp(t, a, b):
    t = t[..., None]
    return np.array(a) * (1 - t) + np.array(b) * t


def cap():
    """A toadstool's skin. Its spots are not here: a projected image stretches and
    cuts them on a curved cap, so the renderer draws them from the volume."""
    # Broad mottling only: fine grain under a bump map reads as noise on a cap.
    mottle = tile_noise(2, 4)
    return ramp(mottle, (0.50, 0.08, 0.05), (0.80, 0.20, 0.09)), 0.35 + 0.05 * mottle, 2.0


def stem():
    """Pale flesh with fine fibres running up it (V is up on a wall)."""
    fibres = tile_noise(4, 8, 0.5)
    streak = streaks(fibres, 0.03)
    blotch = tile_noise(3, 4)
    a = ramp(streak, (0.62, 0.55, 0.42), (0.90, 0.84, 0.70)) * (0.88 + 0.12 * blotch[..., None])
    return a, 0.3 + 0.6 * streak, 2.0


def bark():
    """Dark furrows running up the trunk, with ridges broken into plates."""
    grain = tile_noise(5, 8, 0.6)
    ridge = streaks(grain, 0.05)
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
    run = streaks(tile_noise(3, 8), 0.04)
    t = 0.6 * flow + 0.4 * run
    # Little range: on a curved comb two projections meet, and a wide one shows
    # where they do as patches of another brightness.
    a = ramp(t, (0.80, 0.54, 0.14), (0.92, 0.66, 0.20))
    return a, 0.4 + 0.1 * t, 0.8


def periodic_noise(r, w, h, octaves=5, base=4, persistence=0.55):
    """Like tile_noise for a w x h image, from generator `r`. Periodic in both."""
    acc, amp, total = np.zeros((h, w)), 1.0, 0.0
    for o in range(octaves):
        gw = base * (2 ** o)
        gh = max(2, gw * h // w)
        if gw >= w or gh >= h:
            break
        f = np.fft.fftshift(np.fft.fft2(r.random((gh, gw))))
        pad = np.zeros((h, w), dtype=complex)
        y0, x0 = (h - gh) // 2, (w - gw) // 2
        pad[y0:y0 + gh, x0:x0 + gw] = f
        up = np.real(np.fft.ifft2(np.fft.ifftshift(pad)))
        up = (up - up.min()) / (np.ptp(up) + 1e-9)
        acc += up * amp
        total += amp
        amp *= persistence
    acc /= total
    return (acc - acc.min()) / (np.ptp(acc) + 1e-9)


def periodic_noise_1d(r, w, octaves=6, base=3, persistence=0.55):
    """Periodic fractal noise in [0,1] along one axis of w samples."""
    acc, amp, total = np.zeros(w), 1.0, 0.0
    for o in range(octaves):
        g = base * (2 ** o)
        if g >= w:
            break
        f = np.fft.fftshift(np.fft.fft(r.random(g)))
        pad = np.zeros(w, dtype=complex)
        lo = (w - g) // 2
        pad[lo:lo + g] = f
        up = np.real(np.fft.ifft(np.fft.ifftshift(pad)))
        up = (up - up.min()) / (np.ptp(up) + 1e-9)
        acc += up * amp
        total += amp
        amp *= persistence
    acc /= total
    return (acc - acc.min()) / (np.ptp(acc) + 1e-9)


def bleed(rgb, alpha):
    """Fill the colour under transparent texels from the nearest solid ones, so
    a mip or a bilinear tap across an edge does not pull in black."""
    rgb = rgb.copy()
    have = alpha > 0
    for _ in range(max(rgb.shape[:2])):
        if have.all():
            break
        w = have.astype(float)
        acc, cnt = np.zeros_like(rgb), np.zeros(have.shape)
        for axis, step in ((0, 1), (0, -1), (1, 1), (1, -1)):
            acc += np.roll(rgb * w[..., None], step, axis)
            cnt += np.roll(w, step, axis)
        grow = ~have & (cnt > 0)
        rgb[grow] = acc[grow] / cnt[grow][..., None]
        have = have | grow
    return rgb


def flat_height(h, w):
    """Height for a texture with no relief: the normal comes out straight up."""
    return np.full((h, w), 0.5)


def sky_stars():
    """Near-black with faint nebula and round, soft stars at least ~2 texels
    across, so a bilinear tap neither loses them nor makes them flicker."""
    w = 512
    r = sky_rng
    haze = smooth_step(0.45, 0.85, periodic_noise(r, w, w, 6, 2, 0.6))
    tint = periodic_noise(r, w, w, 3, 2)
    glow = haze[..., None] * ramp(tint, (0.10, 0.04, 0.16), (0.03, 0.10, 0.20))
    img = np.array([0.006, 0.008, 0.02]) + glow * 0.7
    yy, xx = np.mgrid[-5:6, -5:6]
    for _ in range(420):
        cx, cy = r.integers(0, w, 2)
        sigma = 1.1 + 0.9 * r.random() ** 3
        peak = 0.35 + 0.65 * r.random() ** 0.5
        col = ramp(np.array([r.random()]), (0.75, 0.85, 1.0), (1.0, 0.88, 0.7))[0]
        blob = peak * np.exp(-(xx ** 2 + yy ** 2) / (2 * sigma ** 2))
        ys, xs = (cy + yy) % w, (cx + xx) % w
        img[ys, xs] += blob[..., None] * col
    return img, flat_height(w, w), 1.0, None


def sky_clouds():
    """White-grey cloud with soft edges and a grey underside, in alpha."""
    w = 512
    r = sky_rng
    n = periodic_noise(r, w, w, 7, 3, 0.55)
    cover = smooth_step(0.42, 0.72, n)
    shade = periodic_noise(r, w, w, 4, 6)
    col = ramp(shade * 0.7 + (1 - cover) * 0.3, (0.97, 0.98, 1.0), (0.62, 0.66, 0.74))
    return bleed(col, cover), flat_height(w, w), 1.0, cover * 0.92


def sky_ridge():
    """A mountain line: opaque below a ridge that wraps in U, clear above. Two
    ridges, a pale far one and a dark near one, with haze toward each crest."""
    w, h = 512, 256
    r = sky_rng
    y = np.mgrid[0:h, 0:w][0] + 0.5
    detail = periodic_noise(r, w, h, 5, 4, 0.5)
    img, alpha = np.zeros((h, w, 3)), np.zeros((h, w))
    for crest, base_lo, base_hi, amp in (
        (0.22, (0.30, 0.37, 0.50), (0.48, 0.57, 0.71), 0.22),
        (0.45, (0.03, 0.05, 0.09), (0.15, 0.20, 0.30), 0.30),
    ):
        prof = periodic_noise_1d(r, w, 7, 3, 0.62)
        peaks = 1 - np.abs(2 * prof - 1)
        top = h * (crest + amp * (1 - (0.4 * prof + 0.6 * peaks)))
        depth = np.clip((y - top[None, :]) / 110.0, 0, 1)
        haze = (1 - depth) ** 2
        body = ramp(haze, base_lo, base_hi) * (0.85 + 0.3 * detail[..., None])
        # Edge coverage from the crest's sub-texel height: hard, but no staircase.
        cov = np.clip(y - top[None, :] + 0.5, 0, 1)
        img = body * cov[..., None] + img * (1 - cov[..., None])
        alpha = cov + alpha * (1 - cov)
    return bleed(img, alpha), flat_height(h, w), 1.0, alpha


def sky_planet():
    """An Earth-like disc: ocean, green and tan land, polar ice, cloud bands,
    a lit side, a dark limb and a thin blue rim. Not a real map."""
    n, rad = 256, 120.0
    r = sky_rng
    yy, xx = np.mgrid[0:n, 0:n]
    px, py = (xx + 0.5 - n / 2) / rad, -(yy + 0.5 - n / 2) / rad
    d = np.hypot(px, py)
    inside = d < 1
    nz = np.sqrt(np.clip(1 - d ** 2, 0, 1))
    lat = np.arcsin(np.clip(py, -1, 1))
    lon = np.arctan2(px, nz)
    # Equirectangular maps, sampled at each surface point.
    mw, mh = 1024, 512
    land_map = periodic_noise(r, mw, mh, 6, 12, 0.5)
    cloud_map = periodic_noise(r, mw, mh, 6, 6, 0.55)
    my = np.clip(((0.5 - lat / np.pi) * mh).astype(int), 0, mh - 1)
    # Cut by quantile so a third of the surface is land whatever the noise did,
    # and turn the globe to the side with the most land on it.
    sea = np.quantile(land_map[mh // 4:3 * mh // 4], 0.64)
    haze = np.quantile(cloud_map, 0.5)
    column = lambda turn: ((lon / (2 * np.pi) + 0.5) * mw).astype(int) % mw + turn
    best = max(range(0, mw, 32), key=lambda t: (land_map[my, column(t) % mw] > sea)[inside].sum())
    mx = column(best) % mw
    land_n, cloud_n = land_map[my, mx], cloud_map[my, mx]
    land = smooth_step(sea, sea + 0.03, land_n)[..., None]
    ocean = ramp(smooth_step(sea - 0.22, sea, land_n), (0.03, 0.14, 0.42), (0.10, 0.45, 0.62))
    green = ramp(smooth_step(sea + 0.03, sea + 0.25, land_n), (0.12, 0.38, 0.13), (0.50, 0.44, 0.25))
    surface = ocean * (1 - land) + green * land
    ice = smooth_step(1.05, 1.25, np.abs(lat) + 0.15 * (land_n - 0.5))[..., None]
    surface = surface * (1 - ice) + np.array([0.92, 0.95, 1.0]) * ice
    bands = 0.5 + 0.5 * np.cos(lat * 7 + 4 * (cloud_n - 0.5))
    cloud = (smooth_step(haze, haze + 0.2, cloud_n) * (0.35 + 0.65 * bands) * 0.9)[..., None]
    surface = surface * (1 - cloud) + np.array([0.97, 0.98, 1.0]) * cloud
    # Light from the upper left and a little toward the viewer.
    lit = np.clip(px * -0.55 + py * 0.45 + nz * 0.70, 0, 1)
    shade = (0.30 + 0.70 * smooth_step(0.0, 0.6, lit)) * (0.45 + 0.55 * nz ** 0.6)
    rim = ((1 - nz) ** 4 * (0.3 + 0.7 * lit))[..., None] * np.array([0.25, 0.45, 0.85])
    img = surface * shade[..., None] + rim
    alpha = np.clip((1 - d) * rad + 0.5, 0, 1)
    return bleed(np.where(inside[..., None], img, 0), alpha), flat_height(n, n), 1.0, alpha


if __name__ == "__main__":
    for name, fn in [("brick", brick), ("metal", metal), ("rock", rock),
                     ("floor", floor_tile), ("lightpanel", lightpanel)]:
        a, h, s = fn()
        save(name, a, h, s)
    for name, fn in [("bark", bark), ("cap", cap), ("coral", coral),
                     ("stem", stem), ("wax", wax)]:
        a, h, s = fn()
        save(name, a, h, s)
    for name, fn in [("sky-stars", sky_stars), ("sky-clouds", sky_clouds),
                     ("sky-ridge", sky_ridge), ("sky-planet", sky_planet)]:
        a, h, s, alpha = fn()
        save(name, a, h, s, alpha)
