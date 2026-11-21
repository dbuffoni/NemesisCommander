"""Draws the NemesisCommander icon: assets/branding/nemesiscommander.svg (the
tile), nemesiscommander-logo.svg and .png (the logo alone), the PNG sizes and
the two marks for the About window, from one description of the shapes.

    scripts/make-icons.py            (needs node with Playwright's Chromium)

The N is two white pills with a black outline, a black diagonal band and a
blue band (#1F6BFF) between them. The blue edges leave the pills with
tangent arcs (no kinks). Coordinates are in a 343 x 320 box; the N is
centred on (170, 160).
"""
import json
import os
import subprocess
import tempfile

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "branding")
BLUE = "#1F6BFF"
TILE = "#ffffff"

BODY = f"""<g stroke="#000" stroke-width="2" stroke-linejoin="round">
    <rect x="48" y="45" width="80" height="230" rx="40" fill="#000"/>
    <rect x="212" y="45" width="80" height="230" rx="40" fill="#000"/>
    <line x1="88" y1="85" x2="252" y2="235" stroke-width="80" stroke-linecap="round"/>
  </g>
  <path d="M84.00 125.20 A50.0 50.0 0 0 1 156.40 116.55 L181.93 137.98 A25.0 25.0 0 0 0 218.77 132.74 A40 40 0 0 0 256.00 194.80 A50.0 50.0 0 0 1 183.60 203.45 L158.07 182.02 A25.0 25.0 0 0 0 121.23 187.26 A40 40 0 0 0 84.00 125.20Z" fill="{BLUE}"/>
  <g fill="#fff" stroke="#000" stroke-width="2">
    <rect x="48" y="125" width="80" height="150" rx="40"/>
    <rect x="212" y="45" width="80" height="150" rx="40"/>
  </g>"""
CENTRE = (170, 160)
SIDE = 280  # the box around the N used by the logo and the marks


def logo_svg(plate=None, size=256):
    """The logo alone; `plate` is an optional rounded background colour."""
    x, y = CENTRE[0] - SIDE / 2, CENTRE[1] - SIDE / 2
    back = f'<rect x="{x}" y="{y}" width="{SIDE}" height="{SIDE}" rx="56" fill="{plate}"/>\n  ' if plate else ""
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="{x} {y} {SIDE} {SIDE}">
  <title>NemesisCommander</title>
  {back}{BODY}
</svg>
"""


def tile_svg():
    s = 1.3
    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512" viewBox="0 0 512 512">
  <title>NemesisCommander</title>
  <rect width="512" height="512" rx="112" fill="{TILE}"/>
  <g transform="translate(256 256) scale({s}) translate(-{CENTRE[0]} -{CENTRE[1]})">
  {BODY}
  </g>
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
    open(out("nemesiscommander-logo.svg"), "w").write(logo_svg())
    jobs = [(out("nemesiscommander.svg"), out(f"nemesiscommander-{n}.png"), n, True)
            for n in (16, 24, 32, 48, 64, 128, 256, 512)]
    # The logo for the README: on white, 1024 pixels.
    path = os.path.join(work, "logo-white.svg")
    open(path, "w").write(logo_svg(plate="#ffffff", size=1024))
    jobs.append((path, out("nemesiscommander-logo.png"), 1024, False))
    # The marks: on a light theme the logo alone, on a dark one on a white plate
    # (the outline and the band are dark).
    for name, plate in {"light": None, "dark": "#ffffff"}.items():
        path = os.path.join(work, f"mark-{name}.svg")
        open(path, "w").write(logo_svg(plate=plate))
        jobs.append((path, out(f"nemesiscommander-mark-{name}.png"), 256, True))
    script = os.path.join(work, "render.js")
    open(script, "w").write(RENDER)
    subprocess.run(["node", script, json.dumps(jobs)], check=True,
                   env={**os.environ, "NODE_PATH": subprocess.run(
                       ["npm", "root", "-g"], capture_output=True, text=True).stdout.strip()})
