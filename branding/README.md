# Pairnox brand

Masters (approved 2026-11-27, **never edited by hand**; every other logo and icon is made from them):

- `pairnox-logo-icon.svg` the round icon. Used alone for the application icon, window and taskbar, favicon and small sizes.
- `pairnox-logo-wordmark-on-light.svg` icon + wordmark, "Pair" near black. For light backgrounds.
- `pairnox-logo-wordmark-on-dark.svg` icon + wordmark, "Pair" white. For dark backgrounds.

Colours: blue `#1E88E5` (ring, chevrons, "nox"), black `#000000` (right half, thin ring), white `#FFFFFF` (left half), near black `#111418` ("Pair" on light). The text of the wordmark is outlined (Inter ExtraBold), so it needs no font.

Clear space: keep at least a quarter of the icon width free around the icon, and half of it around the wordmark. Do not recolour, rotate, stretch or restyle. Below 33 px the thick small variant is used (made by the script).

Generated files live in `assets/branding` and are made by `scripts/make-icons.py` (PNG 16 to 1024 with real alpha, `pairnox.ico`, `pairnox.icns`, the wordmark PNGs, the symbolic icon). CI runs `scripts/make-icons.py --check`.
The artwork of the former name is not kept in the product; it stays in git history.
