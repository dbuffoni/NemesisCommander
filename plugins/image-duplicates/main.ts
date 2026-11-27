// Image duplicates: finds pictures that are copies of each other, resized or
// saved again (a perceptual "difference hash", `pnxr.imageHash`).
//
//   Find similar pictures          groups the pictures of the folder and of the
//                                  folders below it (or of the selection), says
//                                  how alike they are and which copy to keep.
//   Move the extra copies aside    moves all but the best copy of each group to
//                                  the folder `_similar-extras` (nothing is
//                                  deleted; move them back to undo).
//   Columns "Similar" and "Picture" in the panels: the group, the likeness to
//                                  the best copy of the folder, and the pixels.
//
// Install:  pnxr plugin install image-duplicates

const EXTENSIONS = ["jpg", "jpeg", "png", "gif", "webp", "bmp", "tif", "tiff", "ico"];
/** Hashes that differ in at most this many of 64 bits count as similar (about 90%). */
const MAX_DISTANCE = 6;
const EXTRAS_FOLDER = "_similar-extras";

interface Picture {
  path: string;
  name: string;
  hash: bigint;
  width: number;
  height: number;
  bytes: number;
}

const isPicture = (name: string): boolean => {
  const dot = name.lastIndexOf(".");
  return dot > 0 && EXTENSIONS.includes(name.slice(dot + 1).toLowerCase());
};

const distance = (a: bigint, b: bigint): number => {
  let bits = a ^ b;
  let count = 0;
  while (bits > 0n) {
    count += Number(bits & 1n);
    bits >>= 1n;
  }
  return count;
};

/** How alike two hashes are, in percent (100: the same). */
const likeness = (a: bigint, b: bigint): number => Math.round(((64 - distance(a, b)) / 64) * 100);

const human = (bytes: number): string => {
  const units = ["B", "KiB", "MiB", "GiB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${units[unit]}`;
};

const pixels = (p: Picture): number => p.width * p.height;

/** Hashes `paths`; files that are not pictures are left out. */
async function hashAll(paths: string[]): Promise<Picture[]> {
  const pictures: Picture[] = [];
  for (const path of paths) {
    const info = await pnxr.imageHash(path);
    if (info === null) continue;
    pictures.push({
      path,
      name: pnxr.basename(path),
      hash: BigInt("0x" + info.hash),
      width: info.width,
      height: info.height,
      bytes: info.bytes,
    });
  }
  return pictures;
}

/** The pictures in `roots` and, for folders, below them. */
async function collect(roots: string[]): Promise<string[]> {
  const found: string[] = [];
  const visit = async (path: string, depth: number): Promise<void> => {
    const entry = await pnxr.stat(path);
    if (entry.kind === "file") {
      if (isPicture(entry.name)) found.push(entry.path);
    } else if (entry.kind === "dir" && depth < 32) {
      for (const child of await pnxr.list(path)) {
        if (child.name === EXTRAS_FOLDER) continue;
        await visit(child.path, depth + 1);
      }
    }
  };
  for (const root of roots) await visit(root, 0);
  return found;
}

/** Groups of similar pictures; in each, the best copy (most pixels, then the
 * larger file, then the name) first. Groups are ordered by size. */
function group(pictures: Picture[]): Picture[][] {
  const parent = pictures.map((_, i) => i);
  const root = (i: number): number => {
    while (parent[i] !== i) {
      parent[i] = parent[parent[i]];
      i = parent[i];
    }
    return i;
  };
  for (let i = 0; i < pictures.length; i++) {
    for (let j = i + 1; j < pictures.length; j++) {
      if (distance(pictures[i].hash, pictures[j].hash) <= MAX_DISTANCE) {
        parent[root(i)] = root(j);
      }
    }
  }
  const groups = new Map<number, Picture[]>();
  pictures.forEach((p, i) => {
    const key = root(i);
    groups.set(key, [...(groups.get(key) ?? []), p]);
  });
  const result = [...groups.values()].filter((g) => g.length > 1);
  for (const g of result) {
    g.sort((a, b) => pixels(b) - pixels(a) || b.bytes - a.bytes || a.path.localeCompare(b.path));
  }
  result.sort((a, b) => b.length - a.length || a[0].path.localeCompare(b[0].path));
  return result;
}

const context = pnxr.context;
if (context === null) {
  throw new Error("run this from a panel (Ctrl+X, or the command list F9) or with `pnxr plugin run`");
}

const roots = (): string[] => {
  const chosen = context.selection.length > 0 ? context.selection : context.panel ? [context.panel] : [];
  if (chosen.length === 0) throw new Error("no folder to look in");
  return chosen;
};

const command = context.command;
if (command === "column:similar") {
  // One line per file of the folder, in order: the group of the file and how
  // alike it is to the best copy of its group.
  const pictures = await hashAll(context.selection);
  const groups = group(pictures);
  const lines = new Map<string, string>();
  groups.forEach((g, index) => {
    g.forEach((p, position) => {
      lines.set(p.path, position === 0 ? `#${index + 1} best` : `#${index + 1} ${likeness(p.hash, g[0].hash)}%`);
    });
  });
  for (const path of context.selection) pnxr.print(lines.get(path) ?? "");
} else if (command === "column:picture") {
  const known = new Map((await hashAll(context.selection)).map((p) => [p.path, p]));
  for (const path of context.selection) {
    const p = known.get(path);
    pnxr.print(p ? `${p.width}x${p.height}` : "");
  }
} else if (command === "find" || command === "move-extras") {
  const pictures = await hashAll(await collect(roots()));
  const groups = group(pictures);
  if (groups.length === 0) {
    pnxr.print(`No similar pictures among ${pictures.length} pictures`);
  } else if (command === "find") {
    const extras = groups.reduce((n, g) => n + g.length - 1, 0);
    pnxr.print(`${groups.length} group${groups.length === 1 ? "" : "s"} of similar pictures among ${pictures.length} (${extras} extra copies)`);
    groups.forEach((g, index) => {
      pnxr.print("");
      pnxr.print(`Group ${index + 1}: ${g.length} pictures. Keep the first.`);
      g.forEach((p, position) => {
        const same = position > 0 && p.hash === g[0].hash && p.width === g[0].width && p.height === g[0].height;
        const note = position === 0 ? "keep" : `${likeness(p.hash, g[0].hash)}%${same ? ", same picture" : ""}`;
        pnxr.print(`  ${note.padEnd(20)} ${p.width}x${p.height}, ${human(p.bytes)}  ${p.path}`);
      });
    });
  } else {
    const panel = context.panel;
    if (panel === null) throw new Error("run this from a panel");
    const folder = pnxr.join(panel, EXTRAS_FOLDER);
    if (!(await pnxr.exists(folder))) await pnxr.mkdir(folder);
    let moved = 0;
    for (const g of groups) {
      for (const p of g.slice(1)) {
        let target = pnxr.join(folder, p.name);
        for (let n = 2; await pnxr.exists(target); n++) {
          const dot = p.name.lastIndexOf(".");
          target = pnxr.join(folder, `${p.name.slice(0, dot)} (${n})${p.name.slice(dot)}`);
        }
        await pnxr.rename(p.path, target);
        pnxr.print(`${p.path} -> ${target}`);
        moved += 1;
      }
    }
    pnxr.print(`moved ${moved} extra cop${moved === 1 ? "y" : "ies"} to ${EXTRAS_FOLDER}`);
  }
} else {
  throw new Error(`unknown command ${command}`);
}
