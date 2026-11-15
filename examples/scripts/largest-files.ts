// Prints the largest files below a folder.
//
//   sc run examples/scripts/largest-files.ts --allow-read ~/Downloads -- ~/Downloads 10

const [root = ".", count = "10"] = sc.args;

async function* files(folder: string): AsyncGenerator<sc.Entry> {
  for (const entry of await sc.list(folder)) {
    if (entry.kind === "dir") yield* files(entry.path);
    else if (entry.kind === "file") yield entry;
  }
}

const all: sc.Entry[] = [];
for await (const file of files(root)) all.push(file);
all.sort((a, b) => b.size - a.size);
for (const file of all.slice(0, Number(count))) {
  sc.print(`${(file.size / 1024 / 1024).toFixed(1).padStart(8)} MiB  ${file.path}`);
}
