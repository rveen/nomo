#!/usr/bin/env python3
"""Draw the figures the example worksheets carry, and print them as `.nomo`
resource blocks.

The examples need diagrams, and a diagram lifted from somewhere else is a
licensing question this repository does not want. These are drawn here instead,
from nothing but the standard library, so the bytes in `examples/*.nomo` are as
much this project's own work as the worksheets around them.

Deterministic on purpose: same output on every machine and every run, so a
regenerated figure is a no-op in the diff unless the drawing actually changed.
Uncompressed-deflate PNG, which needs no encoder beyond zlib at level 9 with a
fixed strategy.

    python3 scripts/make-example-figures.py llc
    python3 scripts/make-example-figures.py interlock
"""
import base64
import struct
import sys
import zlib

BG, FG, ACCENT, MUTED = (255, 255, 255), (32, 34, 38), (198, 64, 40), (150, 156, 164)


class Canvas:
    def __init__(self, w, h, bg=BG):
        self.w, self.h = w, h
        self.px = [list(bg) for _ in range(w * h)]

    def dot(self, x, y, c):
        if 0 <= x < self.w and 0 <= y < self.h:
            self.px[y * self.w + x] = list(c)

    def rect(self, x0, y0, x1, y1, c=FG, t=2):
        for i in range(t):
            for x in range(x0, x1 + 1):
                self.dot(x, y0 + i, c)
                self.dot(x, y1 - i, c)
            for y in range(y0, y1 + 1):
                self.dot(x0 + i, y, c)
                self.dot(x1 - i, y, c)

    def fill(self, x0, y0, x1, y1, c):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                self.dot(x, y, c)

    def line(self, x0, y0, x1, y1, c=FG, t=2):
        # Bresenham, thickened square so a horizontal wire and a vertical one
        # come out the same weight.
        dx, dy = abs(x1 - x0), -abs(y1 - y0)
        sx, sy = (1 if x0 < x1 else -1), (1 if y0 < y1 else -1)
        err = dx + dy
        while True:
            for a in range(t):
                for b in range(t):
                    self.dot(x0 + a, y0 + b, c)
            if x0 == x1 and y0 == y1:
                break
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x0 += sx
            if e2 <= dx:
                err += dx
                y0 += sy

    def coil(self, x, y, n, r, c=FG):
        """An inductor: n half-circles along a horizontal wire."""
        for k in range(n):
            cx = x + r + 2 * r * k
            for step in range(181):
                import math

                a = math.radians(step)
                self.line(
                    round(cx - r * math.cos(a)), round(y - r * math.sin(a)),
                    round(cx - r * math.cos(a)), round(y - r * math.sin(a)), c, 2,
                )

    def caps(self, x, y, gap=6, half=11, c=FG):
        """A capacitor: two plates astride the wire."""
        self.line(x, y - half, x, y + half, c, 2)
        self.line(x + gap, y - half, x + gap, y + half, c, 2)

    def png(self):
        raw = b"".join(
            b"\x00" + bytes(v for p in self.px[r * self.w:(r + 1) * self.w] for v in p)
            for r in range(self.h)
        )

        def chunk(tag, data):
            return (
                struct.pack(">I", len(data))
                + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
            )

        return (
            b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", self.w, self.h, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b"")
        )


def llc_tank():
    """The resonant tank: source, Lr, Cr, Lm, and the reflected load."""
    c = Canvas(520, 260)
    top, bot = 70, 190
    c.line(60, top, 60, bot)                      # source
    c.fill(48, 110, 72, 150, BG)
    c.rect(48, 110, 72, 150)
    c.line(60, top, 140, top)
    c.coil(140, top, 4, 14)                       # Lr
    c.line(252, top, 300, top)
    c.caps(300, top)                              # Cr
    c.line(306, top, 380, top)
    c.line(380, top, 380, bot)                    # Lm branch
    c.coil(380, 130, 3, 14)
    c.line(380, top, 460, top)
    c.fill(444, 105, 476, 155, BG)
    c.rect(444, 105, 476, 155, ACCENT)            # reflected load
    c.line(460, top, 460, 105)
    c.line(460, 155, 460, bot)
    c.line(60, bot, 460, bot)
    for x in (196, 300, 460):                     # tick marks under the parts
        c.line(x, bot + 10, x, bot + 18, MUTED, 2)
    return c


def gain_curves():
    """Three gain curves against frequency — the shape the worksheet plots."""
    import math

    c = Canvas(480, 240)
    c.line(56, 200, 450, 200, MUTED)              # axes
    c.line(56, 24, 56, 200, MUTED)
    for q, col in ((0.25, MUTED), (0.45, FG), (0.85, ACCENT)):
        prev = None
        for i in range(395):
            fn = 0.45 + 1.75 * i / 394
            ln = 4.0
            d = math.sqrt(
                (ln * fn * fn - 1) ** 2
                + (fn * fn) * ((fn * fn - 1) ** 2) * ((ln - 1) ** 2) * q * q
            )
            g = (fn * fn) * (ln - 1) / d
            y = 200 - min(174, g * 62)
            p = (56 + i, round(y))
            if prev:
                c.line(prev[0], prev[1], p[0], p[1], col, 2)
            prev = p
    return c


def divider():
    """A two-resistor divider off a rail — the interlock sense front end."""
    c = Canvas(300, 220)
    c.line(40, 40, 250, 40)
    c.line(250, 40, 250, 180)
    c.line(40, 180, 250, 180)
    for y0 in (60, 120):                          # the two resistors
        c.fill(238, y0, 262, y0 + 40, BG)
        c.rect(238, y0, 262, y0 + 40)
    c.line(250, 110, 290, 110, ACCENT)            # the tap
    c.fill(30, 100, 50, 120, BG)
    c.rect(30, 100, 50, 120, MUTED)
    return c


def rc_filter():
    """A series R into a shunt C — the anti-alias filter on the sense line."""
    c = Canvas(300, 160)
    c.line(30, 50, 100, 50)
    c.fill(100, 34, 160, 66, BG)
    c.rect(100, 34, 160, 66)
    c.line(160, 50, 270, 50)
    c.line(210, 50, 210, 78)
    c.caps(199, 92, 22, 0)
    c.line(199, 92, 221, 92)
    c.line(199, 104, 221, 104)
    c.line(210, 104, 210, 130)
    c.line(150, 130, 270, 130)
    return c


def states():
    """Four bars: the four states the interlock line can be found in."""
    c = Canvas(360, 150)
    for i, h in enumerate((28, 96, 60, 122)):
        x = 30 + 84 * i
        col = ACCENT if i == 3 else FG
        c.fill(x, 130 - h, x + 46, 130, col)
    c.line(20, 130, 340, 130, MUTED)
    return c


FIGURES = {
    "llc": [("tank", llc_tank, "520x260"), ("gain", gain_curves, "480x240")],
    "interlock": [
        ("divider", divider, "300x220"),
        ("filter", rc_filter, "300x160"),
        ("states", states, "360x150"),
    ],
}


def main():
    which = sys.argv[1] if len(sys.argv) > 1 else "llc"
    out = []
    for name, draw, _size in FIGURES[which]:
        data = draw().png()
        out.append(f"' image {name} png {len(data)}")
        b = base64.b64encode(data).decode()
        for i in range(0, len(b), 76):
            out.append("'   " + b[i:i + 76])
    print("\n".join(out))


if __name__ == "__main__":
    main()
