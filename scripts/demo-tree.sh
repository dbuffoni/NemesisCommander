#!/usr/bin/env bash
# Fills a folder with a small, realistic home for screenshots and demos:
#   scripts/demo-tree.sh DIR
set -euo pipefail
dir="$1"
h="$dir/home"
mkdir -p "$h"/{Projects/website/css,Projects/website/js,Projects/notes,Projects/scripts,Pictures/2026,Documents,Downloads,Music}

cat > "$h/Projects/website/index.html" <<'HTML'
<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Lake house</title>
<link rel="stylesheet" href="css/site.css"></head>
<body>
  <h1>Lake house</h1>
  <p>Two bedrooms, a wood stove and a jetty. Open from April to October.</p>
  <script src="js/booking.js"></script>
</body>
</html>
HTML
printf 'body { font: 16px/1.5 system-ui; margin: 2rem auto; max-width: 40rem; }\nh1 { color: #2d5d7b; }\n' > "$h/Projects/website/css/site.css"
printf 'document.querySelectorAll("form").forEach((form) => {\n  form.addEventListener("submit", (event) => event.preventDefault());\n});\n' > "$h/Projects/website/js/booking.js"
cat > "$h/Projects/notes/README.md" <<'MD'
# Notes

Ideas and plans, one file per subject.

## This week

- Fix the gutter on the north side
- **Order** firewood (3 m³)
- Call the plumber about the `boiler`

| Room    | Paint  | Litres |
|---------|--------|-------:|
| Kitchen | white  | 5      |
| Hall    | sage   | 2.5    |

```sh
rsync -a ~/Pictures/2026 backup:/photos/
```
MD
printf 'Groceries: milk, eggs, coffee\nGarden: prune the roses in March\n' > "$h/Projects/notes/todo.txt"
printf '#!/bin/sh\n# Back up the photos\nrsync -a "$HOME/Pictures/" /mnt/backup/pictures/\n' > "$h/Projects/scripts/backup.sh"
chmod +x "$h/Projects/scripts/backup.sh"
printf 'const names = (await nmc.list(nmc.context!.panel!)).map((e) => e.name);\nsc.print(names.join("\\n"));\n' > "$h/Projects/scripts/list.ts"
head -c 2400000 /dev/urandom > "$h/Downloads/installer.bin"
head -c 180000 /dev/urandom > "$h/Music/morning.flac"
head -c 95000 /dev/urandom > "$h/Music/evening.flac"
printf 'Invoice 2026-114\nTotal: 1,240.00 EUR\n' > "$h/Documents/invoice-2026-114.txt"
printf 'name,role,city\nAda,engineer,London\nLinus,maintainer,Portland\n' > "$h/Documents/team.csv"
# Pictures: generated with Python if it is there (gradients, so they
# compress like photos and look like something in the viewer).
if command -v python3 >/dev/null; then
  python3 - "$h/Pictures" <<'PY'
import struct, sys, zlib, math
def png(path, w, h, f):
    raw = b"".join(b"\0" + bytes(c for x in range(w) for c in f(x, y)) for y in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d))
    with open(path, "wb") as out:
        out.write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0))
                  + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))
base = sys.argv[1]
png(f"{base}/2026/sunset.png", 480, 320, lambda x, y: (255, int(120 + 100 * y / 320), int(60 + 150 * (1 - y / 320))))
png(f"{base}/2026/lake.png", 480, 320, lambda x, y: (int(40 + 40 * math.sin(x / 30)), int(110 + 60 * y / 320), 200))
png(f"{base}/forest.png", 400, 400, lambda x, y: (30, int(90 + 90 * abs(math.sin((x + y) / 50))), 40))
PY
fi
# An archive to browse.
(cd "$h/Projects" && python3 -c "
import zipfile, os
with zipfile.ZipFile('$h/Downloads/site-backup.zip', 'w', zipfile.ZIP_DEFLATED) as z:
    for root, _, files in os.walk('website'):
        for f in files: z.write(os.path.join(root, f))
")
# Documents, a PDF and a 3D model from nmc-preview's samples.
if command -v cargo >/dev/null; then
  root="$(cd "$(dirname "$0")/.." && pwd)"
  (cd "$root" && cargo run -q -p nmc-preview --example samples -- "$h/Documents" >/dev/null)
  mkdir -p "$h/Projects/models"
  mv "$h/Documents/torus.stl" "$h/Projects/models/"
  rm "$h/Documents/guide.md"
fi
touch -d '2026-11-02 09:15' "$h/Documents/invoice-2026-114.txt" 2>/dev/null || true
