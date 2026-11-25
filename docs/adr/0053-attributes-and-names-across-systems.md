# 0053. Attributes and names across systems

Status: accepted (0.4.0, SC-167)

## Context

Copying between Linux, macOS, Windows and servers loses or breaks things the
user cares about: Windows attributes (read-only, hidden, system), extended
attributes (macOS tags, the quarantine flag, Linux `user.*`), and names that
are different on one system but the same on another (`A`/`a`, NFC/NFD).

## Decision

- `Entry::os_flags` carries the Windows attributes; `SetAttributes` can set
  them. They do not cross the agent protocol.
- Extended attributes are read and written by two optional provider methods
  (`read_xattrs`, `write_xattrs`), advertised in the capabilities. The copy
  plan has `PlanOptions::xattrs`: `Keep` (default), `StripQuarantine`
  (config `strip_quarantine`), `Drop`. The field is `#[serde(default)]`, so
  journals written by 0.3.0 still resume.
- `nmc_vfs::names::NameRules` says when two names are the same file on the
  destination. `usual()` is the local system's habit; remote and archive
  destinations compare exactly. The planner warns about clashes among the
  names it is about to create in one folder.
- Panels hide entries with the Windows hidden/system attribute like dot
  files, and an option hides `.DS_Store` even when hidden files are shown.
- Real macOS/Windows behaviour is only tested in the nightly full tier; the
  Linux tier covers the pure logic and `user.*` attributes.

## Consequences

Failure to set an attribute on the destination never fails the copy.
