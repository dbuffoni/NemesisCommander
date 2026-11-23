// random-file: a template plugin.
//
// It creates one file in the active panel, with a random format (json, xml,
// txt or png) and random content, and prints what it made. Copy this folder,
// rename it and replace the parts marked TEMPLATE with your own work.
//
// A plugin is a folder with two files:
//   plugin.json  what the plugin is called, which commands it adds to the
//                Ctrl+X / F9 list, which locations it may touch (permissions)
//                and its limits.
//   main.ts      this script. NemesisCommander runs it once per command, in
//                its own isolate. There are no imports and no network: only
//                standard JavaScript (ES2022) and the global `nmc`
//                (`nmc run --types` prints every function with its comments).
//
// Try it without touching your files:
//   nmc test examples/plugins/random-file
// Install and use:
//   nmc plugin install examples/plugins/random-file
//   then press Ctrl+X (or F9) in a panel and pick "Create a file with random content".

// ---- 1. What is this run about? -------------------------------------------
// `nmc.context` says which command was picked (`command`, the `id` in
// plugin.json), the two panels' folders (`panel`, `other`) and the selected
// entries (`selection`). It is null when the script is run with `nmc run`.
const context = nmc.context;
if (context === null || context.panel === null) {
  throw new Error("run this from a panel (Ctrl+X) or with `nmc plugin run`");
}

// TEMPLATE: plugins with several commands branch on `context.command` here:
//   if (context.command === "create") { ... } else if (...) { ... }
if (context.command !== "create") {
  throw new Error(`unknown command: ${context.command}`);
}

// ---- 2. Small helpers ------------------------------------------------------
const pick = <T>(items: T[]): T => items[Math.floor(Math.random() * items.length)];
const randomInt = (max: number): number => Math.floor(Math.random() * max);
const randomWord = (): string =>
  pick(["alpha", "bravo", "cedar", "delta", "ember", "falcon", "garnet", "harbor", "iris", "juniper"]);

// ---- 3. One generator per format ------------------------------------------
// Each returns the file content: a string for text formats, bytes for binary
// ones. TEMPLATE: add your own formats here and list them in `formats` below.

const makeJson = (): string =>
  JSON.stringify(
    { id: randomInt(100000), name: randomWord(), tags: [randomWord(), randomWord()], active: Math.random() < 0.5 },
    null,
    2,
  ) + "\n";

const makeXml = (): string =>
  `<?xml version="1.0" encoding="UTF-8"?>\n<item id="${randomInt(100000)}">\n` +
  `  <name>${randomWord()}</name>\n  <value>${randomInt(1000)}</value>\n</item>\n`;

const makeTxt = (): string => {
  const words: string[] = [];
  for (let i = 0; i < 12; i++) words.push(randomWord());
  return words.join(" ") + "\n";
};

// A PNG is a signature followed by chunks (length, type, data, CRC-32). This
// one is a 16x16 image of random coloured blocks, with its pixels stored
// uncompressed inside the zlib stream, so no compression library is needed.
const crcTable = (() => {
  const table: number[] = [];
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table.push(c >>> 0);
  }
  return table;
})();
const crc32 = (bytes: number[]): number => {
  let c = 0xffffffff;
  for (const b of bytes) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
const u32 = (n: number): number[] => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const chunk = (type: string, data: number[]): number[] => {
  const name = [...type].map((ch) => ch.charCodeAt(0));
  return [...u32(data.length), ...name, ...data, ...u32(crc32([...name, ...data]))];
};

const makePng = (): Uint8Array => {
  const size = 16;
  const block = 4; // pixels per coloured block
  const colours: number[][] = [];
  for (let i = 0; i < (size / block) ** 2; i++) colours.push([randomInt(256), randomInt(256), randomInt(256)]);
  // Raw image data: each row starts with filter byte 0, then R G B per pixel.
  const raw: number[] = [];
  for (let y = 0; y < size; y++) {
    raw.push(0);
    for (let x = 0; x < size; x++) raw.push(...colours[Math.floor(y / block) * (size / block) + Math.floor(x / block)]);
  }
  // zlib wrapper around one "stored" (uncompressed) deflate block + Adler-32.
  let a = 1;
  let b = 0;
  for (const v of raw) {
    a = (a + v) % 65521;
    b = (b + a) % 65521;
  }
  const len = raw.length;
  const zlib = [0x78, 0x01, 0x01, len & 255, len >> 8, ~len & 255, (~len >> 8) & 255, ...raw, ...u32(((b << 16) | a) >>> 0)];
  const header = [...u32(size), ...u32(size), 8, 2, 0, 0, 0]; // 8 bits, RGB
  return new Uint8Array([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a,
    ...chunk("IHDR", header),
    ...chunk("IDAT", zlib),
    ...chunk("IEND", []),
  ]);
};

// ---- 4. Choose a format and a name that is free ---------------------------
const formats: { extension: string; make: () => string | Uint8Array }[] = [
  { extension: "json", make: makeJson },
  { extension: "xml", make: makeXml },
  { extension: "txt", make: makeTxt },
  { extension: "png", make: makePng },
];
const format = pick(formats);

let path = "";
do {
  // nmc.join builds paths for any location (local, sftp://, zip://...).
  path = nmc.join(context.panel, `random-${randomInt(100000)}.${format.extension}`);
} while (await nmc.exists(path));

// ---- 5. Write the file -----------------------------------------------------
// writeText for text, writeBytes for binary content. Both need the `write`
// permission for this folder, which plugin.json asks for as "$PANEL".
const content = format.make();
if (typeof content === "string") await nmc.writeText(path, content);
else await nmc.writeBytes(path, content);

// Anything printed is shown to the user when the command ends.
nmc.print(`created ${nmc.basename(path)}`);
