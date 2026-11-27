# 0063. AI providers, the pnxr command and pictures in the terminal

- Status: Accepted
- Date: 2026-11-27

## Context

0.7.1 closes a list of rough edges found while using 0.7.0. Three of them
needed a decision.

1. The AI features (a script, a rename rule) only worked with Anthropic, with a
   key from `ANTHROPIC_API_KEY` or `pnxr ai key`, and a missing key stopped
   with one line.
2. From an AppImage or an app bundle there was no `pnxr` command on the PATH.
3. Pictures in the terminal were drawn with two pixels per cell, which looks
   blocky.

## Decision

**AI providers.** Settings has an AI section (`[ai]`: `default`, and a list of
`providers` with `name`, `kind`, `base_url`, `model`, `key_env`). Anthropic is
built in and always there. `kind` is `anthropic` (the Messages API) or
`openai` (chat completions: OpenAI, OpenRouter, Ollama, LM Studio, any server
that speaks it). Presets exist for OpenRouter, OpenAI, a local server and a
custom one. The section is written only once it differs from the default, so a
settings file that never used it stays readable by older versions.
Keys never go in the settings file (it is exported and shared): each comes from
its provider's environment variable, else from a private file in the settings
folder (`anthropic-api-key` for Anthropic, as before; `ai-key-NAME` for
others). `pnxr ai new|rename` take `--provider` and `--model`; `pnxr ai key
--provider NAME` saves a key; `pnxr ai providers` lists them. For the OpenAI
kind the schema goes into the instructions and the reply is asked as a JSON
object, which every server that speaks that API knows (not all know JSON
schemas). When the default provider needs a key and has none, "Write a script
with AI" and "Multi-rename with AI" first show a dialog that says how to get
one and every way to give it, with a field to paste and save it there.

**The pnxr command.** "Install the pnxr command…" in the Pairnox menu writes a
small launcher named `pnxr` into `~/.local/bin` (or a folder the user types;
`%LOCALAPPDATA%\Pairnox\bin` with a `.cmd` on Windows). It starts the
AppImage with `pnxr` as its first argument, or the `pnxr` program next to the
window. Only files that carry the marker line are replaced or removed. The
same window adds or removes the `pnxr shell-init` line in `~/.bashrc`,
`~/.zshrc` or fish's `config.fish` (between marker lines), and "Quit without
changing the shell's folder" ends the terminal interface without writing the
folder file that `shell-init` reads.

**Pictures in the terminal.** The terminal interface uses the graphics of the
terminal when it has them: kitty graphics (kitty, Ghostty), iTerm2 images
(iTerm2, WezTerm) and sixel (foot, mlterm, Contour, and any terminal when
forced), through the `ratatui-image` crate. The terminal is recognised from
its environment, not asked, because a question would have to be answered while
keys are read; inside tmux or screen graphics are not used. The setting
"Pictures in the terminal" (or `PNXR_IMAGES`) forces kitty, iTerm2, sixel,
blocks or sextants. The fallback shrinks with a Lanczos filter and never
enlarges; sextants (six pixels per cell) are off by default because a font
without them shows empty boxes. The About logo is drawn from its shape (ring,
chevrons, two halves) instead of a shrunk picture, at two sizes, with nothing
drawn around the disc.

## Consequences

Sixel was checked in xterm (`-ti vt340`) in the cloud; kitty and iTerm2 output
comes from the library and is still to be checked in those terminals (card
0.7.2). Kitty needs a terminal with Unicode placeholders; WezTerm gets iTerm2
images for that reason.
