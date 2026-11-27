"""Makes every Pairnox logo and icon file from the three SVG masters in
`branding/`. Nothing is drawn or exported by hand: running this script
again reproduces `assets/branding` byte for byte (given the same renderer).

    scripts/make-icons.py            (needs node with Playwright's Chromium, and Pillow)
    scripts/make-icons.py --check    (CI: the generated files match the masters)

Masters (approved 2026-11-27, never edited):
    branding/pairnox-logo-icon.svg               the round icon
    branding/pairnox-logo-wordmark-on-light.svg  icon + wordmark, "Pair" dark
    branding/pairnox-logo-wordmark-on-dark.svg   icon + wordmark, "Pair" white

Made into assets/branding:
    pairnox.svg, pairnox-wordmark-on-{light,dark}.svg   masters without the provenance metadata
    pairnox-symbolic.svg                                one colour (currentColor), for panels and the terminal
    pairnox-{16,24,32,48,64,128,256,512,1024}.png       icon with real alpha; 16-32 use the small variant
    pairnox-wordmark-on-{light,dark}-1100.png
    pairnox.ico (16-256, Windows) and pairnox.icns (macOS)
"""
import json
import os
import re
import subprocess
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "branding")
OUT = os.path.join(HERE, "..", "assets", "branding")
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
SMALL = 32  # up to this size the thick variant is used


def read(name):
    return open(os.path.join(SRC, name), encoding="utf-8").read()


def clean(svg):
    """Drops the provenance metadata block: it is large and not needed to draw."""
    return re.sub(r"\s*<metadata>.*?</metadata>", "", svg, flags=re.S)


def small_variant(svg):
    """Thicker ring and chevrons, so the icon holds up at 16-32 pixels."""
    svg = svg.replace('r="226"', 'r="206"')
    svg = svg.replace("M256 40 A216 216", "M256 60 A196 196").replace("M256 472 Z", "M256 452 Z")
    svg = svg.replace('stroke-width="28"', 'stroke-width="48"')
    return svg


def symbolic(svg):
    """One colour: the ring, the right half filled, the chevrons knocked out of it."""
    return """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" width="512" height="512">
  <title>Pairnox</title>
  <defs><mask id="m"><rect width="512" height="512" fill="#fff"/>
    <path d="M290 190 L366 256 L290 322" fill="none" stroke="#000" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/></mask></defs>
  <circle cx="256" cy="256" r="240" fill="none" stroke="currentColor" stroke-width="20"/>
  <path d="M256 40 A216 216 0 0 1 256 472 Z" fill="currentColor" mask="url(#m)"/>
  <path d="M222 190 L146 256 L222 322" fill="none" stroke="currentColor" stroke-width="28" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""


RENDER = """
const {chromium} = require('playwright');
(async () => {
  const jobs = JSON.parse(process.argv[2]);
  const browser = await chromium.launch();
  for (const [svg, png, w, h] of jobs) {
    const page = await browser.newPage({viewport: {width: w, height: h}});
    await page.setContent('<style>html,body{margin:0;background:transparent}svg{display:block;width:100vw;height:100vh}</style>'
      + require('fs').readFileSync(svg, 'utf8'));
    await page.screenshot({path: png, omitBackground: true});
  }
  await browser.close();
})();
"""


def check():
    """CI: the SVGs in assets/branding are the masters (no hand edits), every
    size exists and is a square PNG with an alpha channel."""
    from PIL import Image
    icon = clean(read("pairnox-logo-icon.svg"))
    want = {"pairnox.svg": icon, "pairnox-symbolic.svg": symbolic(icon),
            "pairnox-wordmark-on-light.svg": clean(read("pairnox-logo-wordmark-on-light.svg")),
            "pairnox-wordmark-on-dark.svg": clean(read("pairnox-logo-wordmark-on-dark.svg"))}
    bad = [n for n, t in want.items() if open(os.path.join(OUT, n), encoding="utf-8").read() != t]
    for n in SIZES:
        im = Image.open(os.path.join(OUT, f"pairnox-{n}.png"))
        if im.size != (n, n) or im.mode != "RGBA":
            bad.append(f"pairnox-{n}.png")
    for n in ("pairnox.ico", "pairnox.icns", "pairnox-wordmark-on-light-1100.png", "pairnox-wordmark-on-dark-1100.png"):
        if not os.path.exists(os.path.join(OUT, n)):
            bad.append(n)
    if bad:
        raise SystemExit("assets/branding is not what scripts/make-icons.py makes: " + ", ".join(bad))


def main():
    if "--check" in __import__("sys").argv:
        return check()
    os.makedirs(OUT, exist_ok=True)
    out = lambda name: os.path.join(OUT, name)
    work = tempfile.mkdtemp()
    icon = clean(read("pairnox-logo-icon.svg"))
    files = {
        "pairnox.svg": icon,
        "pairnox-wordmark-on-light.svg": clean(read("pairnox-logo-wordmark-on-light.svg")),
        "pairnox-wordmark-on-dark.svg": clean(read("pairnox-logo-wordmark-on-dark.svg")),
        "pairnox-symbolic.svg": symbolic(icon),
    }
    for name, text in files.items():
        open(out(name), "w", encoding="utf-8").write(text)
    small = os.path.join(work, "small.svg")
    open(small, "w", encoding="utf-8").write(small_variant(icon))
    jobs = []
    for n in SIZES:
        jobs.append((small if n <= SMALL else out("pairnox.svg"), out(f"pairnox-{n}.png"), n, n))
    for theme in ("light", "dark"):
        jobs.append((out(f"pairnox-wordmark-on-{theme}.svg"), out(f"pairnox-wordmark-on-{theme}-1100.png"), 1100, 1000))
    script = os.path.join(work, "render.js")
    open(script, "w").write(RENDER)
    root = subprocess.run(["npm", "root", "-g"], capture_output=True, text=True).stdout.strip()
    subprocess.run(["node", script, json.dumps(jobs)], check=True, env={**os.environ, "NODE_PATH": root})

    from PIL import Image
    pics = {n: Image.open(out(f"pairnox-{n}.png")).convert("RGBA") for n in SIZES}
    ico = [16, 24, 32, 48, 64, 128, 256]
    pics[256].save(out("pairnox.ico"), sizes=[(n, n) for n in ico],
                   append_images=[pics[n] for n in ico if n != 256])
    pics[1024].save(out("pairnox.icns"), append_images=[pics[n] for n in (16, 32, 64, 128, 256, 512)])


if __name__ == "__main__":
    main()
