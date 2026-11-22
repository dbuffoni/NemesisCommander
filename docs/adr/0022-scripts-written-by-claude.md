# 0022. `sc ai`: scripts written by Claude, checked by `sc test`

- Status: Accepted
- Date: 2026-11-16

## Context

After ADR 0021 the maintainer asked for the second step: SuperCommander itself
writes scripts and plugins with AI, and tests them.

## Decision

- `sc ai new "description"` (`--plugin` for a plugin) sends one request to
  the Claude Messages API: the system prompt is the guide (`sc run
  --guide`) and `sc.d.ts`; the answer is structured output (`summary`,
  `files`). The files go to a temporary folder and are tested with the
  code of `sc test`. A failed report goes back as the next user message,
  with Claude's previous answer appended unchanged, up to `--tries`
  (default 3).
- What passes is saved: a script in the user's scripts folder (so it is
  in F9, Scripts), a plugin installed (its permissions are asked at first
  use, as for any plugin). Existing names need `--force`; nothing is saved
  when no try passes, and the drafts are kept.
- The user's key: `ANTHROPIC_API_KEY`, or the file `anthropic-api-key` in
  the settings folder, written with mode 600 by `sc ai key`. No account,
  no key in the settings file, nothing sent but the description, the
  guide, the API and the test reports (file names and printed lines of the
  sample files).
- Model `claude-opus-5-5` by default (`--model`), effort `high`, with
  server-side refusal fallbacks (`fallbacks: "default"`). A refusal or a
  too-long answer stops with a message.
- Raw HTTP with the reqwest and rustls already linked for WebDAV and S3
  (there is no official Rust SDK). `SC_AI_URL` points tests at a fake
  server.
- In the terminal UI and the window, Tools, "Write a script with AI…"
  asks for the description and runs `sc ai new` (the window has no V8 and
  runs `sc` anyway, ADR 0020).
- Behind the `ai` feature of `sc`, on by default; agent builds leave it out.

## Consequences

- Writing a script no longer needs an editor or an AI assistant, only a
  key. Each try costs one API request.
- A passing test is not a review: it says the script ran on sample files
  and what it changed. The output says so, and the script is a plain file
  the user can read.

## Addendum (2026-11-16): rename rules

The maintainer asked for AI in the multi-rename too. `sc ai rename "description"
FILES...` asks for a `RenameRule` (structured output with the rule's
fields), checks it with `preview_renames` on the files and sends the error
back (3 tries), then prints the rule (as `sc rename` options, or `--json`)
and the new names; it renames nothing. Only the file names are sent, at
most 40 of them. F9, Files, "Multi-rename with AI…" runs it through `sc`
and shows the explanation, the rule and the new names in the usual
multi-rename confirmation.
