# 0058. The Pairnox logo: three SVG masters, every raster made by a script

- Status: Accepted
- Date: 2026-11-27

## Context

0.5.0 renamed the product but kept the old picture. The maintainer supplied the
Pairnox artwork (a round icon: blue ring, a white and a black half, two blue
chevrons) as JPEGs with a checkerboard baked into the pixels. It was redrawn as
clean SVG and approved on 2026-11-27.

## Decision

- Formerly NemesisCommander / SuperCommander: the old artwork is gone from the
  product (it stays in git history).
- Three masters in `branding/`, never edited: the icon, the wordmark for light
  backgrounds and the wordmark for dark backgrounds.
- `scripts/make-icons.py` makes everything else in `assets/branding`: the SVGs
  without provenance metadata, a one-colour symbolic icon, PNGs from 16 to 1024
  with real transparency (16 to 32 pixels use a thicker variant of the icon),
  the wordmark PNGs, `pairnox.ico` and `pairnox.icns`. Nothing is exported by hand.
- CI runs `scripts/make-icons.py --check`; the grep for the old names also
  fails on `supercommander`.
- The About window and the window icon use the icon (it reads on light and dark
  themes); the README and installers use the icon or the wordmark.

## Consequences

One change to a master plus one script run updates every logo. The exe icon of
Windows and the regenerated manual screenshots need a Windows machine and the
desktop window; they are cards for 0.7.0.
