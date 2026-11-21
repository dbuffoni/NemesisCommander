#!/usr/bin/env python3
"""Draws the NemesisCommander icon: assets/branding/nemesiscommander.svg, the
PNG sizes and the two marks for the About window, from one description of
the shapes.

    scripts/make-icons.py            (needs node with Playwright's Chromium)

The N is a white arch and a purple band across it, as in the artwork
(nemesiscommander-logo.png) the shapes were traced from; the coordinates
below are in that picture's pixels (1536 x 1024).
"""
import json
import os
import subprocess
import tempfile

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "branding")
BACKGROUND = "#0b0b12"
WHITE = "#ececf6"
DARK_ARCH = "#1d1b2e"

ARCH = "M425 240 C425 190 465 165 515 182 L755 345 L592 440 L590 662 L478 744 C445 760 425 745 425 715 Z"
BAND = ("M592 437 C615 380 665 345 718 345 L752 347 L1098 628 C1108 636 1112 645 1110 660 "
        "C1105 715 1050 760 995 753 C975 750 962 745 945 733 L625 447 C612 440 600 438 592 437 Z")
DEFS = """<defs>
    <linearGradient id="band" gradientUnits="userSpaceOnUse" x1="640" y1="400" x2="1080" y2="720">
      <stop offset="0" stop-color="#4b12f0"/>
      <stop offset="1" stop-color="#b21eff"/>
    </linearGradient>
  </defs>"""
# Where the N is in the artwork: centre and size.
CENTRE = (768, 462)
SPAN = (687, 595)


def shapes(arch_color):
    return f'<path d="{ARCH}" fill="{arch_color}"/>\n  <path d="{BAND}" fill="url(#band)"/>'


def tile_svg():
    s = 0.56
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <title>NemesisCommander</title>
  {DEFS}
  <rect width="512" height="512" rx="112" fill="{BACKGROUND}"/>
  <g transform="translate(256 256) scale({s}) translate(-{CENTRE[0]} -{CENTRE[1]})">
  {shapes(WHITE)}
  </g>
</svg>
"""


def mark_svg(arch_color):
    side = max(SPAN) + 70
    x, y = CENTRE[0] - side / 2, CENTRE[1] - side / 2
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="{x} {y} {side} {side}">
  {DEFS}
  {shapes(arch_color)}
</svg>
"""


RENDER = """
const {chromium} = require('playwright');
(async () => {
  const jobs = JSON.parse(process.argv[2]);
  const browser = await chromium.launch();
  for (const [svg, png, size, transparent] of jobs) {
    const page = await browser.newPage({viewport: {width: size, height: size}});
    await page.setContent('<style>html,body{margin:0;background:transparent}svg{display:block;width:100vw;height:100vh}</style>'
      + require('fs').readFileSync(svg, 'utf8'));
    await page.screenshot({path: png, omitBackground: transparent});
  }
  await browser.close();
})();
"""


if __name__ == "__main__":
    os.makedirs(OUT, exist_ok=True)
    out = lambda name: os.path.join(OUT, name)
    work = tempfile.mkdtemp()
    open(out("nemesiscommander.svg"), "w").write(tile_svg())
    marks = {"dark": WHITE, "light": DARK_ARCH}
    jobs = [(out("nemesiscommander.svg"), out(f"nemesiscommander-{n}.png"), n, False)
            for n in (16, 24, 32, 48, 64, 128, 256, 512)]
    for name, color in marks.items():
        path = os.path.join(work, f"mark-{name}.svg")
        open(path, "w").write(mark_svg(color))
        jobs.append((path, out(f"nemesiscommander-mark-{name}.png"), 256, True))
    script = os.path.join(work, "render.js")
    open(script, "w").write(RENDER)
    subprocess.run(["node", script, json.dumps(jobs)], check=True,
                   env={**os.environ, "NODE_PATH": subprocess.run(
                       ["npm", "root", "-g"], capture_output=True, text=True).stdout.strip()})
