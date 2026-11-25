//! Scripts run end to end: TypeScript in, files and printed lines out.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use nmc_script::{Permissions, ScriptError, ScriptOptions, run};
use nmc_vfs::VfsPath;
use tempfile::TempDir;

/// A folder to run scripts in, and what they print.
struct Sandbox {
    _dir: TempDir,
    root: PathBuf,
    printed: Arc<Mutex<Vec<String>>>,
}

impl Sandbox {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        Sandbox { _dir: dir, root, printed: Arc::default() }
    }

    fn options(&self, permissions: Permissions) -> ScriptOptions {
        let printed = self.printed.clone();
        ScriptOptions {
            registry: Arc::new(nmc_providers::default_registry()),
            base: self.root.clone(),
            permissions,
            args: vec!["first".into(), "second".into()],
            output: Arc::new(move |line| printed.lock().unwrap().push(line.to_owned())),
            heap_limit: nmc_script::DEFAULT_HEAP_LIMIT,
            time_limit: None,
            invocation: None,
        }
    }

    /// Permission to read and write the sandbox folder.
    fn own_folder(&self) -> Permissions {
        let mut permissions = Permissions::none();
        let folder = VfsPath::from_local(&self.root).unwrap();
        permissions.allow_read(folder.clone()).allow_write(folder);
        permissions
    }

    async fn run(&self, source: &str, permissions: Permissions) -> Result<(), ScriptError> {
        run("test.ts", source, self.options(permissions)).await
    }

    fn printed(&self) -> Vec<String> {
        self.printed.lock().unwrap().clone()
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }
}

fn write(path: &Path, content: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

#[tokio::test]
async fn typescript_prints_and_sees_its_arguments() {
    let s = Sandbox::new();
    let source = r#"
        const greet = (name: string): string => `hello ${name}`;
        interface Pair { a: number; b: number }
        const pair: Pair = { a: 1, b: 2 };
        nmc.print(greet("world"));
        nmc.print(nmc.args.join(","), pair);
        nmc.print(typeof Deno, nmc.context);
    "#;
    s.run(source, Permissions::none()).await.unwrap();
    assert_eq!(s.printed(), ["hello world", r#"first,second {"a":1,"b":2}"#, "undefined null"]);
}

#[tokio::test]
async fn files_are_read_written_listed_and_moved() {
    let s = Sandbox::new();
    write(&s.path("in/notes.txt"), "one\ntwo\n");
    let source = r#"
        const text = await nmc.readText("in/notes.txt");
        await nmc.mkdir("out");
        await nmc.writeText("out/count.txt", String(text.split("\n").length - 1));
        await nmc.rename("out/count.txt", "out/lines.txt");
        const names = (await nmc.list("out")).map((e) => `${e.name}:${e.kind}:${e.size}`);
        nmc.print(names.join(" "));
        nmc.print(await nmc.exists("out/count.txt"), (await nmc.stat("in")).kind);
        await nmc.remove("in/notes.txt");
    "#;
    s.run(source, s.own_folder()).await.unwrap();
    assert_eq!(fs::read_to_string(s.path("out/lines.txt")).unwrap(), "2");
    assert!(!s.path("in/notes.txt").exists());
    assert_eq!(s.printed(), ["lines.txt:file:1", "false dir"]);
}

#[tokio::test]
async fn copies_use_the_copy_engine() {
    let s = Sandbox::new();
    write(&s.path("photos/a.jpg"), "aaaa");
    write(&s.path("photos/2026/b.jpg"), "bb");
    write(&s.path("backup/photos/a.jpg"), "old");
    let source = r#"
        const first = await nmc.copy(["photos"], "backup", { conflict: "skip" });
        nmc.print(first.done, first.skipped, first.failed.length);
        const second = await nmc.copy(["photos"], "backup", { conflict: "overwrite" });
        nmc.print(second.done, second.bytes);
    "#;
    s.run(source, s.own_folder()).await.unwrap();
    assert_eq!(s.printed(), ["1 1 0", "2 6"]);
    assert_eq!(fs::read_to_string(s.path("backup/photos/a.jpg")).unwrap(), "aaaa");
    assert_eq!(fs::read_to_string(s.path("backup/photos/2026/b.jpg")).unwrap(), "bb");
}

#[tokio::test]
async fn paths_are_joined_and_split_like_in_the_panels() {
    let s = Sandbox::new();
    let source = r#"
        nmc.print(nmc.join("/home/", "/me/", "x.txt"), nmc.basename("/a/b/"), nmc.dirname("/a/b"));
        nmc.print(nmc.dirname("/a"), nmc.dirname("x"), nmc.join("sftp://h/", "dir"));
    "#;
    s.run(source, Permissions::none()).await.unwrap();
    assert_eq!(s.printed(), ["/home/me/x.txt b /a", "/ . sftp://h/dir"]);
}

#[tokio::test]
async fn without_permission_nothing_is_touched() {
    let s = Sandbox::new();
    write(&s.path("secret.txt"), "s");
    let denied = s.run(r#"await nmc.readText("secret.txt");"#, Permissions::none()).await;
    let message = denied.unwrap_err().to_string();
    assert!(message.contains("--allow-read"), "{message}");

    let mut read_only = Permissions::none();
    read_only.allow_read(VfsPath::from_local(&s.root).unwrap());
    let denied = s.run(r#"await nmc.writeText("new.txt", "x");"#, read_only).await;
    assert!(denied.unwrap_err().to_string().contains("--allow-write"));
    assert!(!s.path("new.txt").exists());

    // `..` cannot leave a granted folder.
    let inner = s.path("inner");
    fs::create_dir(&inner).unwrap();
    let mut inner_only = Permissions::none();
    inner_only.allow_read(VfsPath::from_local(&inner).unwrap());
    let escape = s.run(r#"await nmc.readText("inner/../secret.txt");"#, inner_only).await;
    assert!(escape.unwrap_err().to_string().contains("--allow-read"));

    // A script can catch the refusal and go on.
    let source = r#"
        try { await nmc.remove("secret.txt"); } catch (e) { nmc.print(e.name); }
    "#;
    s.run(source, Permissions::none()).await.unwrap();
    assert_eq!(s.printed(), ["PermissionDenied"]);
    assert!(s.path("secret.txt").exists());
}

#[tokio::test]
async fn errors_name_what_went_wrong() {
    let s = Sandbox::new();
    let syntax = s.run("const x: number = ;", Permissions::none()).await;
    assert!(matches!(syntax, Err(ScriptError::Syntax(_))), "{syntax:?}");

    let thrown = s.run(r#"throw new Error("boom");"#, Permissions::none()).await.unwrap_err();
    assert!(matches!(thrown, ScriptError::Failed(_)));
    assert!(thrown.to_string().contains("boom"), "{thrown}");

    let missing = s.run(r#"await nmc.readText("nope.txt");"#, s.own_folder()).await.unwrap_err();
    assert!(missing.to_string().contains("nope.txt"), "{missing}");

    let wrong = s.run(r#"await nmc.copy("a", "b");"#, s.own_folder()).await.unwrap_err();
    assert!(wrong.to_string().contains("array"), "{wrong}");

    let policy = s.run(r#"await nmc.copy([], ".", { conflict: "ask" });"#, s.own_folder()).await;
    assert!(policy.unwrap_err().to_string().contains("conflict"));
}

#[tokio::test]
async fn scripts_cannot_import_modules() {
    let s = Sandbox::new();
    write(&s.path("other.js"), "export const x = 1;");
    let source = r#"import { x } from "./other.js"; nmc.print(x);"#;
    assert!(s.run(source, s.own_folder()).await.is_err());
    assert!(s.printed().is_empty());
}

#[tokio::test]
async fn the_api_cannot_be_replaced() {
    let s = Sandbox::new();
    let source = r#"
        "use strict";
        try { (nmc as any).readText = () => "fake"; } catch { nmc.print("frozen"); }
    "#;
    s.run(source, Permissions::none()).await.unwrap();
    assert_eq!(s.printed(), ["frozen"]);
}

#[tokio::test]
async fn a_script_using_too_much_memory_is_stopped() {
    let s = Sandbox::new();
    let mut options = s.options(Permissions::none());
    options.heap_limit = 32 * 1024 * 1024;
    let source = r#"
        const hoard: string[][] = [];
        while (true) hoard.push(new Array(100000).fill("memory"));
    "#;
    let result = run("hog.ts", source, options).await;
    assert!(matches!(result, Err(ScriptError::OutOfMemory(_))), "{result:?}");
}

#[tokio::test]
async fn a_script_running_too_long_is_stopped() {
    let s = Sandbox::new();
    let limit = std::time::Duration::from_millis(300);
    let mut options = s.options(Permissions::none());
    options.time_limit = Some(limit);
    let busy = run("busy.ts", "while (true) {}", options).await;
    assert!(matches!(busy, Err(ScriptError::TimedOut(_))), "{busy:?}");

    let mut options = s.options(Permissions::none());
    options.time_limit = Some(limit);
    // A promise that never settles ends the event loop: not a time-out.
    let waiting = "await new Promise(() => {});";
    let result = run("idle.ts", waiting, options).await;
    assert!(!matches!(result, Err(ScriptError::TimedOut(_))), "{result:?}");

    let mut options = s.options(Permissions::none());
    options.time_limit = Some(std::time::Duration::from_secs(30));
    run("quick.ts", "nmc.print('done')", options).await.unwrap();
    assert_eq!(s.printed(), ["done"]);
}

#[test]
fn plain_javascript_is_not_transpiled() {
    let js = "const a = 1;";
    assert_eq!(nmc_script::transpile("x.js", js).unwrap(), js);
    let ts = nmc_script::transpile("x.ts", "const a: number = 1;").unwrap();
    assert!(ts.contains("const a = 1"), "{ts}");
    assert!(nmc_script::TYPES.contains("declare"));
}

#[tokio::test]
async fn a_program_that_was_not_granted_does_not_start() {
    let s = Sandbox::new();
    let error =
        s.run(r#"await nmc.run("git", ["--version"]);"#, Permissions::all()).await.unwrap_err();
    assert!(error.to_string().contains("no permission to run"), "{error}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_granted_program_gets_its_arguments_unchanged() {
    let s = Sandbox::new();
    let mut permissions = s.own_folder();
    permissions.allow_run("printf");
    let source = r#"
        const r = await nmc.run("printf", ["[%s]", "a b  'c' \"d\" $HOME"]);
        nmc.print(`${r.code}:${r.stdout}`);
        try { await nmc.run("printf", ["x"], { cwd: "/" }); } catch (e) { nmc.print("outside"); }
        try { await nmc.run("/usr/bin/printf", ["x"]); } catch (e) { nmc.print("path refused"); }
    "#;
    s.run(source, permissions).await.unwrap();
    assert_eq!(s.printed(), ["0:[a b  'c' \"d\" $HOME]", "outside", "path refused"]);
}
