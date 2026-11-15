// Moves each selected file into a folder named after its extension:
// photo.JPG goes to jpg/photo.JPG, notes.txt to txt/notes.txt. Files
// without an extension and folders stay where they are.
//
// Install:  sc plugin install examples/plugins/sort-by-type
// Use:      select files in a panel, press Alt+X and pick the command.

const context = sc.context;
if (context === null || context.panel === null) {
  throw new Error("run this from a panel (Alt+X) or with `sc plugin run`");
}

let moved = 0;
for (const path of context.selection) {
  const entry = await sc.stat(path);
  const dot = entry.name.lastIndexOf(".");
  if (entry.kind !== "file" || dot <= 0) continue;
  const folder = sc.join(context.panel, entry.name.slice(dot + 1).toLowerCase());
  if (!(await sc.exists(folder))) await sc.mkdir(folder);
  await sc.rename(path, sc.join(folder, entry.name));
  moved += 1;
}
sc.print(`moved ${moved} file${moved === 1 ? "" : "s"}`);
