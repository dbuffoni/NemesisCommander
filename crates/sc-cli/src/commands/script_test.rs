//! `sc test`: tries a script or a plugin on sample files in a throwaway
//! folder and reports what it did (ADR 0021).
//!
//! The report is meant for the author and for AI assistants that write
//! scripts: they run `sc test --json`, read what broke or which files
//! changed, and fix the script until it passes.

#[cfg(not(feature = "scripting"))]
pub async fn test(_context: &sc_app::AppContext, _args: crate::cli::TestArgs) -> super::CmdResult {
    Err("scripts are not part of this build (server builds leave them out)".into())
}

#[cfg(feature = "scripting")]
pub use engine::test;

#[cfg(feature = "scripting")]
mod engine {
    use std::collections::BTreeMap;
    use std::fs;
    use std::hash::{Hash, Hasher};
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use sc_app::AppContext;
    use sc_script::plugin::{Plugin, Requested};
    use sc_script::{Invocation, Permissions, ScriptOptions};
    use sc_vfs::VfsPath;
    use serde::Serialize;

    use crate::cli::TestArgs;
    use crate::commands::CmdResult;

    /// Files the test panel holds when no fixture is given: a little of
    /// everything a file script meets.
    const SAMPLES: &[(&str, &str)] = &[
        ("notes.txt", "shopping list\nmilk\nbread\n"),
        ("empty.log", ""),
        ("photo.JPG", "\u{ff}\u{d8}\u{ff} not really a photo"),
        ("data.csv", "name,size\na,1\nb,2\n"),
        ("docs/readme.md", "# Readme\n\nSome text.\n"),
        ("docs/old/draft.txt", "an old draft\n"),
    ];

    #[derive(Serialize)]
    struct Report {
        target: String,
        kind: &'static str,
        passed: bool,
        checks: Vec<Check>,
        runs: Vec<Run>,
        /// Where the test folders are, with --keep.
        #[serde(skip_serializing_if = "Option::is_none")]
        folder: Option<String>,
        /// Things the test changed from a real run.
        notes: Vec<String>,
    }

    #[derive(Serialize)]
    struct Check {
        name: &'static str,
        status: Status,
        #[serde(skip_serializing_if = "String::is_empty")]
        message: String,
    }

    #[derive(Serialize, Clone, Copy, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    enum Status {
        Passed,
        Failed,
        Skipped,
    }

    #[derive(Serialize)]
    struct Run {
        /// The plugin command; `null` for a script.
        command: Option<String>,
        passed: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        output: Vec<String>,
        seconds: f64,
        /// Paths relative to the run's folder (`panel/...`, `other/...`).
        created: Vec<String>,
        removed: Vec<String>,
        changed: Vec<String>,
    }

    /// One script to try: its file and how to run it.
    struct Target {
        main: PathBuf,
        /// The plugin, or `None` for a script.
        plugin: Option<Plugin>,
    }

    pub async fn test(context: &AppContext, args: TestArgs) -> CmdResult {
        let root = tempfile::Builder::new().prefix("sc-test-").tempdir()?;
        let mut report = Report {
            target: args.target.clone(),
            kind: "script",
            passed: false,
            checks: Vec::new(),
            runs: Vec::new(),
            folder: None,
            notes: Vec::new(),
        };
        // Permissions compare real paths (macOS keeps temp in /private/var).
        let folder = fs::canonicalize(root.path())?;
        if let Some(target) = load(&args, &mut report) {
            check_source(&target, &folder, &mut report);
            if report.checks.iter().all(|check| check.status != Status::Failed) {
                run_all(context, &args, &target, &folder, &mut report).await?;
            }
        }
        report.passed = !report.runs.is_empty()
            && report.runs.iter().all(|run| run.passed)
            && report.checks.iter().all(|check| check.status != Status::Failed);
        if args.keep {
            report.folder = Some(root.keep().display().to_string());
        }
        if args.json {
            crate::out!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print_text(&report);
        }
        Ok(if report.passed { ExitCode::SUCCESS } else { ExitCode::from(1) })
    }

    /// The script or plugin named on the command line; a failed check
    /// when it cannot be loaded.
    fn load(args: &TestArgs, report: &mut Report) -> Option<Target> {
        let path = Path::new(&args.target);
        if path.is_dir() {
            report.kind = "plugin";
            match Plugin::load(path) {
                Ok(plugin) => {
                    for id in &args.command {
                        if plugin.command(id).is_none() {
                            report.checks.push(failed("manifest", format!("no command {id:?}")));
                            return None;
                        }
                    }
                    report.checks.push(passed("manifest"));
                    Some(Target {
                        main: plugin.dir.join(&plugin.manifest.main),
                        plugin: Some(plugin),
                    })
                }
                Err(error) => {
                    report.checks.push(failed("manifest", error.to_string()));
                    None
                }
            }
        } else if path.is_file() {
            Some(Target { main: path.to_path_buf(), plugin: None })
        } else {
            report.checks.push(failed("syntax", format!("{}: not a file or folder", args.target)));
            None
        }
    }

    /// Syntax (always) and types (with `tsc`, when installed).
    fn check_source(target: &Target, root: &Path, report: &mut Report) {
        let name = target.main.display().to_string();
        let source = match fs::read_to_string(&target.main) {
            Ok(source) => source,
            Err(error) => {
                report.checks.push(failed("syntax", format!("{name}: {error}")));
                return;
            }
        };
        match sc_script::transpile(&name, &source) {
            Ok(_) => report.checks.push(passed("syntax")),
            Err(error) => {
                report.checks.push(failed("syntax", error.to_string()));
                return;
            }
        }
        report.checks.push(type_check(&target.main, &root.join("types")));
    }

    /// Checks the script against the `sc` declarations with the
    /// TypeScript compiler; skipped when there is none (`SC_TSC` names
    /// another program).
    fn type_check(script: &Path, folder: &Path) -> Check {
        let program = std::env::var("SC_TSC")
            .unwrap_or_else(|_| if cfg!(windows) { "tsc.cmd" } else { "tsc" }.into());
        let prepare = || -> std::io::Result<PathBuf> {
            fs::create_dir_all(folder)?;
            let declarations = folder.join("sc.d.ts");
            fs::write(&declarations, sc_script::TYPES)?;
            let config = serde_json::json!({
                "compilerOptions": {
                    "noEmit": true, "strict": true, "target": "es2022", "lib": ["es2022"],
                    "module": "esnext", "moduleDetection": "force", "types": [],
                    "skipLibCheck": true, "allowJs": true
                },
                "files": [declarations, fs::canonicalize(script)?],
            });
            let path = folder.join("tsconfig.json");
            fs::write(&path, config.to_string())?;
            Ok(path)
        };
        let config = match prepare() {
            Ok(config) => config,
            Err(error) => return failed("types", error.to_string()),
        };
        let output = std::process::Command::new(&program)
            .arg("-p")
            .arg(&config)
            .arg("--pretty")
            .arg("false")
            .output();
        match output {
            Err(_) => Check {
                name: "types",
                status: Status::Skipped,
                message: format!("{program} not found (npm install -g typescript)"),
            },
            Ok(output) if output.status.success() => passed("types"),
            Ok(output) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                failed("types", text.trim().to_owned())
            }
        }
    }

    /// Runs the script once, or each plugin command, each in fresh folders.
    async fn run_all(
        context: &AppContext,
        args: &TestArgs,
        target: &Target,
        root: &Path,
        report: &mut Report,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let source = fs::read_to_string(&target.main)?;
        let name = target.main.display().to_string();
        let commands: Vec<Option<String>> = match &target.plugin {
            None => vec![None],
            Some(plugin) if args.command.is_empty() => {
                plugin.manifest.commands.iter().map(|c| Some(c.id.clone())).collect()
            }
            Some(_) => args.command.iter().cloned().map(Some).collect(),
        };
        if let Some(plugin) = &target.plugin {
            report.notes.extend(ungranted(&plugin.manifest.permissions));
        }
        for (index, command) in commands.into_iter().enumerate() {
            let folder = root.join(format!("run-{}", index + 1));
            let (panel, other) = (folder.join("panel"), folder.join("other"));
            fs::create_dir_all(&other)?;
            match &args.fixture {
                Some(fixture) => copy_tree(Path::new(fixture), &panel)?,
                None => write_samples(&panel)?,
            }
            let (panel, other) = (fs::canonicalize(panel)?, fs::canonicalize(other)?);
            let selection: Vec<VfsPath> = if args.select.is_empty() {
                let mut names: Vec<_> =
                    fs::read_dir(&panel)?.flatten().map(|entry| entry.file_name()).collect();
                names.sort();
                names
                    .iter()
                    .map(|n| VfsPath::from_local(panel.join(n)))
                    .collect::<Result<_, _>>()?
            } else {
                args.select
                    .iter()
                    .map(|n| VfsPath::from_local(panel.join(n)))
                    .collect::<Result<_, _>>()?
            };
            let (panel_path, other_path) =
                (VfsPath::from_local(&panel)?, VfsPath::from_local(&other)?);

            let (permissions, script_args, time_limit, heap_limit) = match &target.plugin {
                Some(plugin) => (
                    plugin_permissions(
                        &plugin.manifest.permissions,
                        &panel_path,
                        &other_path,
                        &selection,
                    ),
                    Vec::new(),
                    Duration::from_secs(plugin.manifest.limits.timeout.max(1)),
                    plugin.manifest.limits.memory.max(16) * 1024 * 1024,
                ),
                None => {
                    let mut permissions = Permissions::none();
                    permissions.allow_write(VfsPath::from_local(&folder)?);
                    let text = |p: &Path| p.display().to_string();
                    let fill = |a: &String| {
                        a.replace("$PANEL", &text(&panel)).replace("$OTHER", &text(&other))
                    };
                    let script_args = if args.args.is_empty() {
                        vec![text(&panel)]
                    } else {
                        args.args.iter().map(fill).collect()
                    };
                    let limit = Duration::try_from_secs_f64(args.timeout).map_err(|_| {
                        format!("--timeout {}: not a number of seconds", args.timeout)
                    })?;
                    (permissions, script_args, limit, sc_script::DEFAULT_HEAP_LIMIT)
                }
            };
            let printed: Arc<Mutex<Vec<String>>> = Arc::default();
            let sink = printed.clone();
            let options = ScriptOptions {
                registry: context.registry.clone(),
                base: panel.clone(),
                permissions,
                args: script_args,
                output: Arc::new(move |line| sink.lock().expect("output").push(line.to_owned())),
                heap_limit,
                time_limit: Some(time_limit),
                invocation: Some(Invocation::new(
                    command.as_deref().unwrap_or("run"),
                    Some(&panel_path),
                    Some(&other_path),
                    &selection,
                )),
            };
            let before = snapshot(&folder);
            let started = Instant::now();
            let outcome = sc_script::run(&name, &source, options).await;
            let seconds = (started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0;
            let after = snapshot(&folder);
            let output = printed.lock().expect("output").clone();
            let mut run = Run {
                command,
                passed: outcome.is_ok(),
                error: outcome.err().map(|e| e.to_string()),
                output,
                seconds,
                created: Vec::new(),
                removed: Vec::new(),
                changed: Vec::new(),
            };
            compare(&before, &after, &mut run);
            report.runs.push(run);
        }
        Ok(())
    }

    /// A plugin's request with its placeholders filled in with the test
    /// folders. Other locations are not granted in a test ([`ungranted`]).
    fn plugin_permissions(
        requested: &Requested,
        panel: &VfsPath,
        other: &VfsPath,
        selection: &[VfsPath],
    ) -> Permissions {
        let mut permissions = Permissions::none();
        let expand = |location: &str| -> Vec<VfsPath> {
            match location {
                "$PANEL" => vec![panel.clone()],
                "$OTHER" => vec![other.clone()],
                "$SELECTION" => selection.to_vec(),
                _ => Vec::new(),
            }
        };
        for location in &requested.read {
            for path in expand(location) {
                permissions.allow_read(path);
            }
        }
        for location in &requested.write {
            for path in expand(location) {
                permissions.allow_write(path);
            }
        }
        permissions
    }

    /// What a plugin asks for that the test does not grant.
    fn ungranted(requested: &Requested) -> Vec<String> {
        let mut notes = Vec::new();
        if requested.all {
            notes.push(
                "the plugin asks for all files: the test grants only $PANEL, $OTHER and $SELECTION"
                    .into(),
            );
        }
        for location in requested.read.iter().chain(&requested.write) {
            if !["$PANEL", "$OTHER", "$SELECTION"].contains(&location.as_str()) {
                notes.push(format!("{location} is not granted during the test"));
            }
        }
        notes
    }

    fn write_samples(panel: &Path) -> std::io::Result<()> {
        for (name, text) in SAMPLES {
            let path = panel.join(name);
            fs::create_dir_all(path.parent().expect("a parent"))?;
            fs::write(path, text)?;
        }
        Ok(())
    }

    fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    /// Every entry below `folder`: relative path to a fingerprint of its
    /// contents (`None` for folders).
    fn snapshot(folder: &Path) -> BTreeMap<String, Option<u64>> {
        fn walk(base: &Path, dir: &Path, found: &mut BTreeMap<String, Option<u64>>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                let name =
                    path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('\\', "/");
                if path.is_dir() {
                    found.insert(name, None);
                    walk(base, &path, found);
                } else {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    fs::read(&path).unwrap_or_default().hash(&mut hasher);
                    found.insert(name, Some(hasher.finish()));
                }
            }
        }
        let mut found = BTreeMap::new();
        walk(folder, folder, &mut found);
        found
    }

    fn compare(
        before: &BTreeMap<String, Option<u64>>,
        after: &BTreeMap<String, Option<u64>>,
        run: &mut Run,
    ) {
        for (path, print) in after {
            match before.get(path) {
                None => run.created.push(path.clone()),
                Some(old) if old != print => run.changed.push(path.clone()),
                Some(_) => {}
            }
        }
        run.removed = before.keys().filter(|path| !after.contains_key(*path)).cloned().collect();
    }

    fn passed(name: &'static str) -> Check {
        Check { name, status: Status::Passed, message: String::new() }
    }

    fn failed(name: &'static str, message: String) -> Check {
        Check { name, status: Status::Failed, message }
    }

    fn print_text(report: &Report) {
        crate::out!("sc test {} ({})", report.target, report.kind);
        for check in &report.checks {
            let status = match check.status {
                Status::Passed => "passed",
                Status::Failed => "FAILED",
                Status::Skipped => "skipped",
            };
            match check.message.as_str() {
                "" => crate::out!("  {:<9} {status}", check.name),
                message => crate::out!("  {:<9} {status}: {message}", check.name),
            }
        }
        for note in &report.notes {
            crate::out!("  note: {note}");
        }
        for run in &report.runs {
            let label =
                run.command.as_deref().map(|c| format!("run {c}")).unwrap_or_else(|| "run".into());
            let status = if run.passed { "passed" } else { "FAILED" };
            crate::out!("  {label:<9} {status} in {:.2} s", run.seconds);
            if let Some(error) = &run.error {
                crate::out!("    error: {error}");
            }
            for line in &run.output {
                crate::out!("    | {line}");
            }
            for (title, paths) in
                [("created", &run.created), ("removed", &run.removed), ("changed", &run.changed)]
            {
                for path in paths {
                    crate::out!("    {title} {path}");
                }
            }
        }
        if let Some(folder) = &report.folder {
            crate::out!("  folder kept: {folder}");
        }
        crate::out!("{}", if report.passed { "passed" } else { "FAILED" });
    }
}
