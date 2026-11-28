# 0064. HEIC and RAW pictures

- Status: Accepted
- Date: 2026-11-28

## Context

Card SC-208 asked for HEIC and RAW pictures in *Find similar pictures*. The
`image` crate reads neither. HEIC is HEVC video coding in a HEIF container,
RAW is a different format for every camera maker.

## Decision

- **HEIC/HEIF** is decoded with `heifer`, a decoder written in Rust (no C
  library, no `unsafe`, MIT or Apache-2.0, tested against the HEVC
  conformance streams). It applies rotation, mirroring and cropping, so a
  portrait photo is upright. Decoder only: its encoder is used in tests.
  Alternatives looked at: `libheif` (C and C++, LGPL, a native library to ship
  on every system), `heic` (AGPL or commercial), and the system decoders
  (ImageIO, WIC), which differ per system.
- **RAW** files are not developed. Almost every RAW file carries a JPEG
  preview; the biggest JPEG found inside the file is used, for the picture
  hash and for the viewer. A RAW and its JPEG therefore match as copies.
- The code is one small crate, `pnxr-picture`, used by the picture hash
  (`pnxr.imageHash`) and by the viewer, so both read the same formats. HEIC is
  recognised by its `ftyp` brand, RAW by the file name.
- Files above 128 MiB are not decoded.

## Consequences

- HEVC is covered by patents that the license of the code does not grant.
  Pairnox ships a software decoder, as other open source programs do; anyone
  distributing Pairnox where that matters should take their own advice. If it
  becomes a problem, the decoder can be removed behind this crate alone.
- The crates are young (0.x). A damaged file is refused (tested with cut and
  corrupted data); real photos from phones should be tried before a release.
- 10-bit and HDR photos are shown as 8-bit sRGB. RAW previews are as good as
  the camera made them (sometimes small).
- The binary grows by the size of the decoder; the footprint budget in CI
  watches it.
