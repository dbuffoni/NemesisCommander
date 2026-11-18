// Prints the largest files below a folder.
//
//   nmc run examples/scripts/largest-files.ts --allow-read ~/Downloads -- ~/Downloads 10

const [root = ".", count = "10"] = nmc.args;

async function* files(folder: string): AsyncGenerator<nmc.Entry> {
  for (const entry of await nmc.list(folder)) {
    if (entry.kind === "dir") yield* files(entry.path);
    else if (entry.kind === "file") yield entry;
  }
}

const all: nmc.Entry[] = [];
for await (const file of files(root)) all.push(file);
all.sort((a, b) => b.size - a.size);
for (const file of all.slice(0, Number(count))) {
  nmc.print(`${(file.size / 1024 / 1024).toFixed(1).padStart(8)} MiB  ${file.path}`);
}
