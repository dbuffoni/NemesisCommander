//! Plugins from folder to command: install, grant, run with the panels'
//! context.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use pnxr_script::plugin::{self, Context, Grants, PluginError};
use pnxr_vfs::VfsPath;
use tempfile::TempDir;

const MANIFEST: &str = r#"{
  "name": "stamp",
  "version": "1.0.0",
  "description": "Writes a list of the selection into the other panel",
  "commands": [
    { "id": "list", "title": "List the selection" },
    { "id": "peek", "title": "Read outside the selection" }
  ],
  "permissions": { "read": ["$SELECTION"], "write": ["$OTHER"] },
  "limits": { "memory": 64, "timeout": 5 }
}"#;

const MAIN: &str = r#"
const ctx = pnxr.context!;
if (ctx.command === "list") {
  const names = ctx.selection.map((path: string) => pnxr.basename(path));
  await pnxr.writeText(pnxr.join(ctx.other!, "selection.txt"), names.join("\n"));
  pnxr.print(`listed ${names.length}`);
} else {
  await pnxr.readText(pnxr.join(ctx.panel!, "private.txt"));
}
"#;

struct World {
    _dir: TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        for folder in ["source", "left", "right", "plugins"] {
            fs::create_dir(root.join(folder)).unwrap();
        }
        fs::write(root.join("source/plugin.json"), MANIFEST).unwrap();
        fs::write(root.join("source/main.ts"), MAIN).unwrap();
        fs::write(root.join("left/a.txt"), "a").unwrap();
        fs::write(root.join("left/b.txt"), "b").unwrap();
        fs::write(root.join("left/private.txt"), "p").unwrap();
        World { _dir: dir, root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    fn context(&self) -> Context {
        let local = |p: PathBuf| VfsPath::from_local(p).unwrap();
        Context {
            panel: Some(local(self.path("left"))),
            other: Some(local(self.path("right"))),
            selection: vec![local(self.path("left/a.txt")), local(self.path("left/b.txt"))],
            home: None,
        }
    }
}

async fn run(
    plugin: &plugin::Plugin,
    command: &str,
    context: &Context,
) -> (Result<(), PluginError>, Vec<String>) {
    let printed = Arc::new(Mutex::new(Vec::new()));
    let sink = printed.clone();
    let output: pnxr_script::Output =
        Arc::new(move |line| sink.lock().unwrap().push(line.to_owned()));
    let registry = Arc::new(pnxr_providers::default_registry());
    let result = plugin.run(command, context, registry, output).await;
    let lines = printed.lock().unwrap().clone();
    (result, lines)
}

#[tokio::test]
async fn an_installed_plugin_runs_its_commands_on_the_panels() {
    let w = World::new();
    let installed = plugin::install(&w.path("source"), &w.path("plugins")).unwrap();
    assert_eq!(installed.dir, w.path("plugins/stamp"));
    let (plugins, problems) = plugin::discover(&w.path("plugins"));
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(plugins, std::slice::from_ref(&installed));

    let (result, printed) = run(&installed, "list", &w.context()).await;
    result.unwrap();
    assert_eq!(printed, ["listed 2"]);
    assert_eq!(fs::read_to_string(w.path("right/selection.txt")).unwrap(), "a.txt\nb.txt");

    // The panel's other files were not asked for.
    let (result, _) = run(&installed, "peek", &w.context()).await;
    let error = result.unwrap_err().to_string();
    assert!(error.contains("private.txt") && error.contains("permission"), "{error}");

    let (result, _) = run(&installed, "missing", &w.context()).await;
    assert!(matches!(result, Err(PluginError::NotFound(_))));
}

#[tokio::test]
async fn the_manifest_limits_apply() {
    let w = World::new();
    fs::write(w.path("source/main.ts"), "while (true) {}").unwrap();
    let manifest = MANIFEST.replace(r#""timeout": 5"#, r#""timeout": 1"#);
    fs::write(w.path("source/plugin.json"), manifest).unwrap();
    let installed = plugin::install(&w.path("source"), &w.path("plugins")).unwrap();
    let (result, _) = run(&installed, "list", &w.context()).await;
    assert!(
        matches!(
            result,
            Err(PluginError::Script { error: pnxr_script::ScriptError::TimedOut(_), .. })
        ),
        "{result:?}"
    );
}

#[test]
fn grants_are_remembered_until_the_request_changes() {
    let w = World::new();
    let grants_file = w.path("config/plugin-grants.json");
    let installed = plugin::install(&w.path("source"), &w.path("plugins")).unwrap();
    let mut grants = Grants::load(&grants_file).unwrap();
    assert!(!grants.is_granted(&installed));
    grants.grant(&installed).unwrap();
    assert!(Grants::load(&grants_file).unwrap().is_granted(&installed));

    // An update that asks for more needs a new grant.
    let wider = MANIFEST.replace(r#""read": ["$SELECTION"]"#, r#""read": ["$SELECTION", "$HOME"]"#);
    fs::write(w.path("source/plugin.json"), wider).unwrap();
    let updated = plugin::install(&w.path("source"), &w.path("plugins")).unwrap();
    assert!(!Grants::load(&grants_file).unwrap().is_granted(&updated));

    let mut grants = Grants::load(&grants_file).unwrap();
    grants.grant(&updated).unwrap();
    grants.revoke("stamp").unwrap();
    assert!(!Grants::load(&grants_file).unwrap().is_granted(&updated));
}

#[test]
fn broken_plugins_are_reported_not_fatal() {
    let w = World::new();
    plugin::install(&w.path("source"), &w.path("plugins")).unwrap();
    fs::create_dir(w.path("plugins/broken")).unwrap();
    fs::write(w.path("plugins/broken/plugin.json"), "{ not json").unwrap();
    fs::write(w.path("plugins/stray-file"), "").unwrap();
    let (plugins, problems) = plugin::discover(&w.path("plugins"));
    assert_eq!(plugins.len(), 1);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].contains("broken"), "{problems:?}");
    assert!(plugin::discover(Path::new("/no/such/folder")).0.is_empty());
}
