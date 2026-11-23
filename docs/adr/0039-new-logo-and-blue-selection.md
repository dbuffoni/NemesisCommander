# 0039. New logo (blue band) and blue selection

- Status: Accepted
- Date: 2026-11-21

## Context

The logo was not good enough. The maintainer supplied a new N: two white
pills with a black outline, a black diagonal band and a band between them,
first purple, then asked blue. He asked the logo to be used to rebrand the
app and the same blue, transparent, for selected rows.

## Decision

- The shapes are redrawn in `scripts/make-icons.py`; the blue band's edges
  are arcs tangent to the pills (concave where it leaves a pill, convex
  where it ends on one), so the junctions have no kinks. Blue is `#1F6BFF`.
- Outputs: `nemesiscommander.svg` and the PNG sizes (white rounded tile,
  transparent corners), `nemesiscommander-logo.svg` and `.png` (README), and
  the two About marks. The black parts vanish on a dark theme, so the dark
  mark sits on a white rounded plate.
- Selected rows in the GUI use the logo's blue at 32 % (dark theme) or 22 %
  (light) opacity, a blue mark at the left edge and a light blue text
  (`selectionTint`, `selectedText`, `brandBlue` in `Main.qml`). The other
  accent colour (gold: headings, git changes, recording) is unchanged.
  The terminal marks selected entries in light blue (no transparency there).
- Manual screenshots (`docs/manual/images`) still show the old colours until
  `scripts/screenshots.sh` is run with a built `nmc-gui`.

## Consequences

The blue is only approximate to the supplied picture (hand-traced); the SVG is
the master. Revisit if the maintainer supplies vector artwork.

## Update (same day)

The maintainer found the S-shaped blue band too busy and asked for a simple bar. The
logo is now a straight blue bar (#1F6BFF, 38 units wide) centred on the black
diagonal, inside a black frame that follows the outline of the N, passing
under the two pills. Same files, same script.
