#!/usr/bin/env python3
"""Draws the NemesisCommander icon: assets/branding/nemesiscommander.svg and
the PNG sizes, from one description of the shapes.

    scripts/make-icons.py            (needs Pillow)

The icon is an N made of a white arch and a purple band across it. The full logo
(nemesiscommander-logo.png) is the artwork it was drawn from.
"""
import math
import os
from PIL import Image, ImageDraw, ImageChops

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "branding")
SIZE = 512
BACKGROUND = "#0b0b12"
WHITE = "#ececf6"
GRADIENT = ("#4b12f0", "#b21eff")

# The arch: corner points and the radius each corner is rounded with. The
# right side of it is hidden under the band, which leaves the dark notch.
ARCH = [((67, 92), 44), ((236, 198), 6), ((157, 243), 2), ((157, 366), 4), ((67, 415), 26)]
# The band: a thick line with round ends, from where it starts to where
# it ends and how wide it is.
BAND = ((207, 238), (403, 378), 84)


def rounded(points):
    """Path commands and sampled outline of a polygon with rounded corners."""
    n = len(points)
    commands, outline = [], []
    for i, (p, r) in enumerate(points):
        a, b = points[i - 1][0], points[(i + 1) % n][0]

        def toward(q):
            d = math.dist(p, q)
            k = min(r, d / 2) / d
            return (p[0] + (q[0] - p[0]) * k, p[1] + (q[1] - p[1]) * k)

        s, e = toward(a), toward(b)
        commands.append(("M" if i == 0 else "L") + f"{s[0]:.1f} {s[1]:.1f}")
        commands.append(f"Q{p[0]} {p[1]} {e[0]:.1f} {e[1]:.1f}")
        for t in range(0, 9):
            u = t / 8
            outline.append(((1 - u) ** 2 * s[0] + 2 * u * (1 - u) * p[0] + u * u * e[0],
                            (1 - u) ** 2 * s[1] + 2 * u * (1 - u) * p[1] + u * u * e[1]))
    return " ".join(commands) + "Z", outline


def svg():
    arch, _ = rounded(ARCH)
    (x1, y1), (x2, y2), width = BAND
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{SIZE}" height="{SIZE}" viewBox="0 0 {SIZE} {SIZE}">
  <title>NemesisCommander</title>
  <defs>
    <linearGradient id="band" gradientUnits="userSpaceOnUse" x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">
      <stop offset="0" stop-color="{GRADIENT[0]}"/>
      <stop offset="1" stop-color="{GRADIENT[1]}"/>
    </linearGradient>
  </defs>
  <rect width="{SIZE}" height="{SIZE}" rx="112" fill="{BACKGROUND}"/>
  <path d="{arch}" fill="{WHITE}"/>
  <line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="url(#band)" stroke-width="{width}" stroke-linecap="round"/>
</svg>
"""


def png(scale=4, tile=True, arch_color=WHITE):
    big = SIZE * scale
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1), 112 * scale, fill=255)
    if tile:
        image.paste(Image.new("RGBA", (big, big), BACKGROUND), mask=mask)
    draw = ImageDraw.Draw(image)
    _, arch = rounded(ARCH)
    draw.polygon([(x * scale, y * scale) for x, y in arch], fill=arch_color)
    (x1, y1), (x2, y2), width = BAND
    band = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(band)
    d.line([(x1 * scale, y1 * scale), (x2 * scale, y2 * scale)], fill=255, width=width * scale)
    for x, y in ((x1, y1), (x2, y2)):
        r = width * scale / 2
        d.ellipse((x * scale - r, y * scale - r, x * scale + r, y * scale + r), fill=255)
    # The gradient runs along the band.
    gradient = Image.new("RGBA", (big, big))
    pixels = gradient.load()
    dx, dy = x2 - x1, y2 - y1
    length = dx * dx + dy * dy
    c0 = tuple(int(GRADIENT[0][i:i + 2], 16) for i in (1, 3, 5))
    c1 = tuple(int(GRADIENT[1][i:i + 2], 16) for i in (1, 3, 5))
    row = [None] * big
    for py in range(0, big):
        for px in range(0, big):
            t = ((px / scale - x1) * dx + (py / scale - y1) * dy) / length
            t = max(0.0, min(1.0, t))
            pixels[px, py] = tuple(int(c0[k] + (c1[k] - c0[k]) * t) for k in range(3)) + (255,)
    image.paste(gradient, mask=ImageChops.multiply(band, mask if tile else Image.new("L", (big, big), 255)))
    return image.resize((SIZE, SIZE), Image.LANCZOS)


def mark(arch_color):
    """The N alone on a transparent background, cropped square: for the
    About window, which sits on a light or a dark theme."""
    image = png(tile=False, arch_color=arch_color)
    box = image.getbbox()
    side = max(box[2] - box[0], box[3] - box[1]) + 24
    cx, cy = (box[0] + box[2]) // 2, (box[1] + box[3]) // 2
    square = image.crop((cx - side // 2, cy - side // 2, cx - side // 2 + side, cy - side // 2 + side))
    return square.resize((256, 256), Image.LANCZOS)


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, "nemesiscommander.svg"), "w") as f:
        f.write(svg())
    icon = png()
    for size in (16, 24, 32, 48, 64, 128, 256, 512):
        icon.resize((size, size), Image.LANCZOS).save(os.path.join(OUT, f"nemesiscommander-{size}.png"))
    # For a dark and for a light theme (the arch changes, the band does not).
    mark(WHITE).save(os.path.join(OUT, "nemesiscommander-mark-dark.png"))
    mark("#1d1b2e").save(os.path.join(OUT, "nemesiscommander-mark-light.png"))
