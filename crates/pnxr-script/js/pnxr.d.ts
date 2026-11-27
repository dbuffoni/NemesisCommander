// Types of the Pairnox scripting API (`@pnxr/sdk`), available in every
// script as the global `pnxr`. Print this file with `pnxr run --types` and put
// it next to your scripts for completion and checking in your editor.
//
// Locations are strings: local paths (relative to the folder the script
// runs in) or URIs such as sftp://host/dir, s3://bucket/key or
// zip:///archive.zip!/inside. A script may only touch the locations its
// permissions allow (`pnxr run --allow-read`, `--allow-write`).

declare namespace pnxr {
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

  /** Where a plugin command was started. */
  interface Context {
    /** The command's id, as in the plugin's manifest. */
    command: string;
    /** The folder shown in the active panel. */
    panel: string | null;
    /** The folder shown in the other panel. */
    other: string | null;
    /** The selected entries, or the entry under the cursor. */
    selection: string[];
  }

  /** What `copy` does when a destination exists. */
  type Conflict =
    | "fail"
    | "overwrite"
    | "overwrite-if-newer"
    | "overwrite-if-different-size"
    | "skip"
    | "keep-both";

  /** The Pairnox version running the script. */
  const version: string;
  /** Arguments given after the script name. */
  const args: string[];
  /** In a plugin command, where it was started; `null` in `pnxr run`. */
  const context: Context | null;

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
  /** What `imageHash` says about a picture. */
  interface ImageHash {
    /** A perceptual hash: 16 hexadecimal digits. Copies of a picture that were
     * resized or saved again differ from it in a few of the 64 bits. */
    hash: string;
    width: number;
    height: number;
    /** Size of the file in bytes. */
    bytes: number;
  }
  /** The perceptual hash of a picture (PNG, JPEG, GIF, WebP, BMP, TIFF, ICO up to
   * 48 MiB), or `null` when the file is not one (or is a flat one). */
  function imageHash(path: string): Promise<ImageHash | null>;
  /** Writes (replaces) a file with UTF-8 text. */
  function writeText(path: string, text: string): Promise<void>;
  /** Writes (replaces) a file with raw bytes, for binary formats such as images. */
  function writeBytes(path: string, bytes: Uint8Array): Promise<void>;
  /** Creates a folder (its parent must exist). */
  function mkdir(path: string): Promise<void>;
  /** Renames or moves within one location. */
  function rename(from: string, to: string): Promise<void>;
  /** Removes a file or an empty folder. */
  function remove(path: string): Promise<void>;
  /** What a program started with `run` did. */
  interface RunReport {
    /** Exit code (-1 when the program was ended by a signal). */
    code: number;
    stdout: string;
    stderr: string;
  }
  /** Starts a program named in the plugin's `permissions.run` (a bare name
   *  looked up on the PATH, no shell: `args` reach it unchanged) and waits
   *  for it. Throws when the program is not granted or not installed. */
  function run(program: string, args?: string[], options?: { cwd?: string }): Promise<RunReport>;
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
