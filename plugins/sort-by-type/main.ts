// Moves each selected file into a folder named after its extension:
// photo.JPG goes to jpg/photo.JPG, notes.txt to txt/notes.txt. Files
// without an extension and folders stay where they are.
//
// Install:  pnxr plugin install plugins/sort-by-type
// Use:      select files in a panel, press Ctrl+X and pick the command.

const context = pnxr.context;
if (context === null || context.panel === null) {
  throw new Error("run this from a panel (Ctrl+X) or with `pnxr plugin run`");
}

let moved = 0;
for (const path of context.selection) {
  const entry = await pnxr.stat(path);
  const dot = entry.name.lastIndexOf(".");
  if (entry.kind !== "file" || dot <= 0) continue;
  const folder = pnxr.join(context.panel, entry.name.slice(dot + 1).toLowerCase());
  if (!(await pnxr.exists(folder))) await pnxr.mkdir(folder);
  await pnxr.rename(path, pnxr.join(folder, entry.name));
  moved += 1;
}
pnxr.print(`moved ${moved} file${moved === 1 ? "" : "s"}`);
