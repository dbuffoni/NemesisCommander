# 0056: Destructive confirmations

Status: accepted (0.4.0, SC-140)

## Decision

Whatever cannot be undone asks first, in the same window in the terminal and
the desktop interface:

- Deleting files permanently: Enter means **no**, `y` means yes (unchanged).
- Moving files to the trash: Enter means yes (it can be undone).
- Removing a saved connection from the drive list (Delete): asks, Enter means
  **no**, because the entry may hold a login.
- Removing a bookmark or a saved search (Delete), and emptying the basket
  (Shift+Delete): ask, Enter means yes; the list opens again afterwards.
- Taking one path out of the basket (Delete) asks nothing: the files are not
  touched and adding the path again is one key.

## Why

A key held down or pressed twice in a row must not lose data nobody can get
back. The question costs one key for entries that are cheap to recreate, and
Enter defaults to "no" where the loss is a secret or a file.
