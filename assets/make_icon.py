#!/usr/bin/env python3
"""Renders the RustRouter icon (PCB traces on a dark board) without external libraries.

Writes rustrouter.png (256x256), rustrouter-64.rgba (raw RGBA for the window icon) and
rustrouter.icns (macOS) next to this script: python3 assets/make_icon.py
"""
import math, os, struct, zlib

HERE = os.path.dirname(os.path.abspath(__file__))

BG = (24, 30, 40)
EDGE = (70, 82, 100)
RED = (235, 40, 40)
BLUE = (60, 110, 255)
PAD = (240, 200, 60)

# Geometry in a 0..1 coordinate system: (kind, color, params)
TRACES = [
    (RED, 0.07, [(0.22, 0.30), (0.45, 0.30), (0.62, 0.47), (0.80, 0.47)]),
    (RED, 0.07, [(0.22, 0.72), (0.40, 0.72), (0.55, 0.57)]),
    (BLUE, 0.07, [(0.22, 0.51), (0.34, 0.51), (0.52, 0.69), (0.80, 0.69)]),
    (BLUE, 0.07, [(0.55, 0.57), (0.55, 0.30), (0.80, 0.30)]),
]
PADS = [(0.22, 0.30), (0.22, 0.51), (0.22, 0.72), (0.80, 0.30), (0.80, 0.47), (0.80, 0.69)]
VIAS = [(0.55, 0.57)]


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def color_at(x, y):
    """The color of the point (x, y) in 0..1 or None outside the icon."""
    r, m = 0.18, 0.04  # corner radius, margin
    qx = max(abs(x - 0.5) - (0.5 - m - r), 0.0)
    qy = max(abs(y - 0.5) - (0.5 - m - r), 0.0)
    d = math.hypot(qx, qy) - r
    if d > 0:
        return None
    c = EDGE if d > -0.025 else BG
    for col, w, pts in TRACES:
        for (ax, ay), (bx, by) in zip(pts, pts[1:]):
            if seg_dist(x, y, ax, ay, bx, by) <= w / 2:
                c = col
    for px, py in PADS:
        if abs(x - px) <= 0.065 and abs(y - py) <= 0.065:
            c = PAD
    for vx, vy in VIAS:
        dd = math.hypot(x - vx, y - vy)
        if dd <= 0.06:
            c = PAD if dd > 0.027 else BG
    return c


def render(size, ss=4):
    out = bytearray()
    for j in range(size):
        for i in range(size):
            acc = [0, 0, 0, 0]
            for sj in range(ss):
                for si in range(ss):
                    c = color_at((i + (si + 0.5) / ss) / size, (j + (sj + 0.5) / ss) / size)
                    if c is not None:
                        acc[0] += c[0]; acc[1] += c[1]; acc[2] += c[2]; acc[3] += 255
            n = ss * ss
            a = acc[3] / n
            if a > 0:
                out += bytes([round(acc[0] * 255 / acc[3]), round(acc[1] * 255 / acc[3]), round(acc[2] * 255 / acc[3]), round(a)])
            else:
                out += b"\0\0\0\0"
    return bytes(out)


def png(size, rgba):
    raw = b"".join(b"\0" + rgba[y * size * 4:(y + 1) * size * 4] for y in range(size))
    def chunk(t, b):
        return struct.pack(">I", len(b)) + t + b + struct.pack(">I", zlib.crc32(t + b) & 0xFFFFFFFF)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def main():
    images = {s: render(s) for s in (64, 128, 256, 512)}
    with open(os.path.join(HERE, "rustrouter.png"), "wb") as f:
        f.write(png(256, images[256]))
    with open(os.path.join(HERE, "rustrouter-64.rgba"), "wb") as f:
        f.write(images[64])
    entries = b""
    for typ, s in ((b"ic07", 128), (b"ic08", 256), (b"ic09", 512)):
        data = png(s, images[s])
        entries += typ + struct.pack(">I", len(data) + 8) + data
    with open(os.path.join(HERE, "rustrouter.icns"), "wb") as f:
        f.write(b"icns" + struct.pack(">I", len(entries) + 8) + entries)


if __name__ == "__main__":
    main()
