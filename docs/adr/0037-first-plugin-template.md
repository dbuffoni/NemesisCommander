# 0037. A template plugin, and `nmc.writeBytes` for binary files

- Status: Accepted
- Date: 2026-11-20

## Context

Card SC-104 asks for a first simple plugin that developers can copy as a
template: it creates a file with random content chosen among json, xml, txt
and png, with well-commented code and placeholders. The only example plugin
(`sort-by-type`) moves files, and the script API could only write UTF-8 text,
so a png could not be written.

## Decision

- `examples/plugins/random-file` adds the command "Create a file with random
  content" (category Files). It writes into the active panel (`$PANEL`, the
  only permission it asks for), picks a free name `random-N.ext` and prints it.
  `main.ts` is numbered in sections (context, helpers, one generator per
  format, choosing, writing) and marks with `TEMPLATE` what to replace.
- The png is built in the script (signature, IHDR, one IDAT with stored
  deflate blocks, IEND, own CRC-32 and Adler-32), because plugins cannot
  import libraries. It is a 16x16 image of random coloured blocks.
- New API `nmc.writeBytes(path, Uint8Array)`, same permission and write
  options as `writeText`.
- `the_examples_work` installs the plugin, runs it 12 times and checks that
  every file is valid for its extension.

## Consequences

- Plugins can now produce any binary file; reading binary content is still
  only possible through `copy`.
