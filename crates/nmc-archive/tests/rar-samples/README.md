# RAR sample archives

Small RAR archives from libarchive's own test suite (libarchive 3.8.1,
`libarchive/test/*.rar.uu`, decoded), under libarchive's BSD-2-Clause licence.
The project does not ship any tool that creates RAR files.

| File | What it holds |
| --- | --- |
| `test_read_format_rar.rar` | RAR 4: two files, a folder, a symbolic link |
| `test_read_format_rar5_compressed.rar` | RAR 5, compressed, one 1200-byte file |
| `test_read_format_rar5_multiple_files_solid.rar` | RAR 5, solid, four 4096-byte files |
| `test_read_format_rar5_solid.rar` | RAR 5, solid, seven files |
| `test_read_format_rar5_encrypted.rar` | RAR 5, encrypted (cannot be read) |
| `test_read_format_rar4_solid_encrypted.rar` | RAR 4, solid, encrypted (cannot be read) |
