# 0043. Agent protocol experience and dialogs

- Status: Accepted
- Date: 2026-11-22

## Context

The maintainer found eleven problems while using the agent (`nmc serve`) and the
dialogs.

## Decision

1. **Install asks first, then shows every step.** Typing the server opens a
   question that lists what will be done (Enter / y installs). The install runs
   in a log window that grows line by line, and ends with a plain verdict;
   on success Enter saves the connection.
2. **Copy between two servers asks about existing files.** The servers copy
   alone and cannot ask, so the user is asked once before the copy
   (`TransferManager::ask_before_server_copy`) and the answer (overwrite, skip,
   keep both) is used for the whole copy. Server errors name the server, the
   route and the status code (`route::describe`).
3. **Full path in copy/move questions**: the answer of a place is shown whole
   under the field (TUI and window); the field scrolls with the cursor.
4. **Back**: Shift+Tab returns to the previous question of a series (the
   questions are kept in `App::history`; the hint shows the key).
5. **Dialogs**: wide enough for long paths and wrapped, never cut; checked with
   10, 16 and 24 pt fonts. The transfer queue is newest first, three rows a job.
6. **Route of a transfer**: each job says how the data travels (server to server
   direct/reverse, through this computer, uploaded, downloaded, inside one server).
7. **Missing server**: opening an `scs://` location that does not answer, or has
   no certificates yet, offers the install (once per host and session).
8. **Windows keep their size**: fixed width (a share of the window), reserved
   rows for the lines under a field, fixed height for logs and filtered lists.
9. **Binary files** open as an information sheet (`nmc-preview/binary.rs`:
   dictionary of signatures and extensions, header details of ELF, PE, Mach-O,
   ZIP, gzip, SQLite, Java, WebAssembly, fonts, tar, ISO; entropy; strings).
   F4 / h shows the hex dump.
10. **Logs**: every command run on the server is written to the log with its
    output; connecting has a 10 s timeout (it waited 1080 s before) and, when
    nothing answers, the server is asked for the service state, listening ports
    and journal.
11. **Top bar**: on a saved connection the title is only the path inside it;
    the drive label already names the server.

## Consequences

The window (QML) could not be built in the cloud; its dialogs were checked with
the Qt `qml` tool against a stand-in window (screenshots in the PR).
