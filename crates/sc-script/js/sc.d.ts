// Types of the SuperCommander scripting API (`@sc/sdk`), available in every
// script as the global `sc`. Print this file with `sc run --types` and put
// it next to your scripts for completion and checking in your editor.
//
// Locations are strings: local paths (relative to the folder the script
// runs in) or URIs such as sftp://host/dir, s3://bucket/key or
// zip:///archive.zip!/inside. A script may only touch the locations its
// permissions allow (`sc run --allow-read`, `--allow-write`).

declare namespace sc {
  /** A file, folder or link. */
  interface Entry {
    /** Last component of the path. */
    name: string;
    /** The full location, usable with every function here. */
    path: string;
    kind: "file" | "dir" | "symlink" | "other";
    /** Size in bytes (0 for folders). */
    size: number;
    /** Modification time in milliseconds since 1970, if known. */
    modified: number | null;
  }

  /** What `copy` did. Folders are created as needed and not counted. */
  interface CopyReport {
    /** Files copied. */
    done: number;
    /** Files left alone because of the conflict policy. */
    skipped: number;
    /** One message per item that failed. */
    failed: string[];
    bytes: number;
  }

  /** What `copy` does when a destination exists. */
  type Conflict =
    | "fail"
    | "overwrite"
    | "overwrite-if-newer"
    | "overwrite-if-different-size"
    | "skip"
    | "keep-both";

  /** The SuperCommander version running the script. */
  const version: string;
  /** Arguments given after the script name. */
  const args: string[];

  /** Prints values on standard output, separated by spaces. */
  function print(...values: unknown[]): void;

  /** The entries of a folder (without `.` and `..`). */
  function list(path: string): Promise<Entry[]>;
  /** Details of one entry; rejects when it does not exist. */
  function stat(path: string): Promise<Entry>;
  /** Whether something exists at `path`. */
  function exists(path: string): Promise<boolean>;
  /** A whole file as UTF-8 text. */
  function readText(path: string): Promise<string>;
  /** Writes (replaces) a file with UTF-8 text. */
  function writeText(path: string, text: string): Promise<void>;
  /** Creates a folder (its parent must exist). */
  function mkdir(path: string): Promise<void>;
  /** Renames or moves within one location. */
  function rename(from: string, to: string): Promise<void>;
  /** Removes a file or an empty folder. */
  function remove(path: string): Promise<void>;
  /** Copies files and folders into the folder `destination`, with the
   *  same engine as the panels (resume, verification, any location). */
  function copy(
    sources: string[],
    destination: string,
    options?: { conflict?: Conflict },
  ): Promise<CopyReport>;

  /** Joins path parts with `/`. */
  function join(...parts: string[]): string;
  /** The last component of a path. */
  function basename(path: string): string;
  /** Everything but the last component. */
  function dirname(path: string): string;
}
