# 0052. Windows paths: drives, UNC shares, long paths, reserved names

- Status: Accepted
- Date: 2026-11-25
- Extends: [0003](0003-path-model.md)

## Decision

- **Form in a `VfsPath`.** A drive path `C:\dir` is `/C:/dir` (as before). A
  UNC share `\\server\share\dir` (and the verbatim `\\?\UNC\server\share\dir`)
  is `/UNC/server/share/dir`. Only the backslash form is read as UNC, so
  `//dir/x` and a folder called `UNC` on Linux or macOS are ordinary paths:
  nothing changes there. The conversions are pure functions in
  `nmc_vfs::winpath`, tested on every system; only the Windows build applies
  them to real paths (`to_local_path`).
- **Long paths.** A native path of 240 characters or more is given to Windows
  with the `\\?\` prefix (`\\?\C:\...`, `\\?\UNC\server\share\...`), which lifts
  the 260-character limit. Paths in the VFS are already normalized (no `.`,
  no `..`), as the prefix requires.
- **Names.** On Windows, creating, writing, renaming or copying to a name
  that Windows cannot hold (a device name such as `CON`, `NUL.txt`, `COM1`,
  a trailing dot or space, `< > : " | ? *`, control characters) fails with an
  `InvalidPath` error that says why, instead of creating something else.
  `winpath::check_name` is the one rule. Reading is not checked.
- **Directory links.** `create_symlink` makes a folder link when the target
  is a folder (relative to the link), a file link otherwise. A folder link or
  junction is removed with the folder call, never touching its target.
  Junctions are listed as links.
- Mapped drives are drive letters and need nothing. Listing UNC shares of
  other machines is not part of this decision.

## Tests

Parsing and name rules: unit tests everywhere. Real file system (reserved
names, a path over 260 characters, the administrative share
`\\localhost\C$\...`, folder links): tests of the local provider compiled on
Windows only, run in the nightly full tier.
