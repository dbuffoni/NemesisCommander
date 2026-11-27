#!/usr/bin/env python3
"""Third-party notices (SC-147).

  scripts/third_party.py --check            fail on a dependency whose licence is not allowed
  scripts/third_party.py -o FILE            write THIRD-PARTY-NOTICES for a release

The Rust dependencies come from `cargo metadata`; the licence of each is an SPDX
expression and one of its alternatives (an `OR` branch) must use only allowed
licences. Qt and libarchive are described by hand below. Release packages ship
the file and the About window points to it.
"""
import argparse
import glob
import hashlib
import json
import os
import re
import subprocess
import sys

ALLOWED = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause",
    "BSD-3-Clause", "ISC", "Zlib", "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0",
    "0BSD", "MPL-2.0", "MPL-2.0+", "BSL-1.0", "Unlicense", "bzip2-1.0.6",
    "CDLA-Permissive-2.0", "WTFPL",
}
# Licences found in no dependency and refused on purpose: a new crate that
# brings one needs a decision first.
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def metadata():
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    data = json.loads(out)
    members = set(data["workspace_members"])
    return [p for p in data["packages"] if p["id"] not in members]


def normalise(expression):
    """`A/B` and `A / B` are `A OR B`; `+` after MPL stays."""
    return re.sub(r"\s*/\s*", " OR ", expression)


def alternatives(expression):
    """The OR branches, each a list of licences that all apply (AND)."""
    text = normalise(expression).replace("(", " ( ").replace(")", " ) ")
    tokens = text.split()

    def parse_or(pos):
        branches, pos = parse_and(pos)
        while pos < len(tokens) and tokens[pos] == "OR":
            more, pos = parse_and(pos + 1)
            branches += more
        return branches, pos

    def parse_and(pos):
        combos = [[]]
        while True:
            if tokens[pos] == "(":
                inner, pos = parse_or(pos + 1)
                pos += 1  # the ")"
            else:
                name = tokens[pos]
                pos += 1
                if pos < len(tokens) and tokens[pos] == "WITH":
                    name += " WITH " + tokens[pos + 1]
                    pos += 2
                inner = [[name]]
            combos = [c + i for c in combos for i in inner]
            if pos < len(tokens) and tokens[pos] == "AND":
                pos += 1
                continue
            return combos, pos

    return parse_or(0)[0]


def acceptable(expression):
    return any(all(name in ALLOWED for name in branch) for branch in alternatives(expression))


def check(packages):
    bad = [(p["name"], p["version"], p["license"]) for p in packages
           if not p["license"] or not acceptable(p["license"])]
    for name, version, license_ in bad:
        print(f"third-party: {name} {version} has licence {license_!r}, which is not allowed",
              file=sys.stderr)
    if bad:
        print("Review the licence and add it to ALLOWED in scripts/third_party.py only if "
              "it is compatible with MIT releases.", file=sys.stderr)
        return 1
    print(f"{len(packages)} dependencies, all with allowed licences")
    return 0


def licence_texts(package):
    folder = os.path.dirname(package["manifest_path"])
    names = glob.glob(os.path.join(folder, "LICEN[CS]E*")) + glob.glob(
        os.path.join(folder, "COPYING*")) + glob.glob(os.path.join(folder, "UNLICENSE*"))
    texts = []
    for name in sorted(set(names)):
        if os.path.isfile(name):
            with open(name, encoding="utf-8", errors="replace") as f:
                texts.append(f.read().strip())
    return texts


QT = """\
## Qt 6 (the window)

The desktop window uses Qt 6 under the GNU Lesser General Public License,
version 3 (https://www.gnu.org/licenses/lgpl-3.0.html). Qt is linked
dynamically: the Qt libraries are separate files in the package (in the
AppImage, inside it), not part of the `pnxr-gui` program. You may replace them
with your own build of Qt that is binary compatible, and relink or run the
program against it. The corresponding Qt source code is available from
https://download.qt.io/official_releases/qt/ (the version is in the package's
file names and in Help > About Qt). The terminal program `pnxr` does not use Qt.
"""

LIBARCHIVE = """\
## libarchive (reading RAR archives)

Release builds read RAR archives with libarchive, built into the program from
the source in `crates/pnxr-libarchive/vendor` with every optional library
switched off. libarchive is under the BSD-2-Clause licence (some files under
other BSD-style licences, see its COPYING):

"""

ASSETS = """\
## Logo, icons and documentation

The files in `assets/` and the documentation are released under the MIT
licence of the project, like the code.
"""


def notices(packages):
    out = ["# Third-party notices", "",
           "Pairnox is released under the MIT licence, Copyright (c) 2026 Daniele Buffoni. "
           "It includes or links the software below. Each is under its own licence.", "",
           QT, LIBARCHIVE]
    with open(os.path.join(ROOT, "crates/pnxr-libarchive/vendor/COPYING"), encoding="utf-8") as f:
        out += ["```", f.read().strip(), "```", "", ASSETS]
    out += ["## Rust libraries", "",
            f"{len(packages)} crates from crates.io, built into the programs. "
            "A crate under several licences (`A OR B`) is used under the first allowed one.", ""]
    groups = {}
    for p in sorted(packages, key=lambda p: (p["name"], p["version"])):
        for text in licence_texts(p)[:2] or [""]:
            key = hashlib.sha256(text.encode()).hexdigest()
            groups.setdefault(key, (text, []))[1].append(p)
    out += ["### Crates", ""]
    for p in sorted(packages, key=lambda p: (p["name"], p["version"])):
        out.append(f"- {p['name']} {p['version']}: {p['license']}")
    out += ["", "### Licence texts", ""]
    for text, users in sorted(groups.values(), key=lambda g: -len(g[1])):
        names = ", ".join(sorted({f"{p['name']} {p['version']}" for p in users}))
        out += [f"#### Used by: {names}", "", "```", text or "(the crate ships no licence file; see its repository)", "```", ""]
    return "\n".join(out) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true")
    parser.add_argument("-o", "--output")
    args = parser.parse_args()
    packages = metadata()
    status = check(packages)
    if args.output and status == 0:
        with open(args.output, "w", encoding="utf-8") as f:
            f.write(notices(packages))
        print(f"wrote {args.output}")
    return status


if __name__ == "__main__":
    sys.exit(main())
