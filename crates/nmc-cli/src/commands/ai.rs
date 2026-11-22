//! `nmc ai`: scripts and plugins written by Claude (ADR 0022).
//!
//! `nmc ai new "description"` asks the Claude API for the files, tries them
//! with `nmc test` (the same report a person or an AI assistant gets), and
//! sends the report back until the test passes or the tries run out. A
//! script that passes goes to the user's scripts folder, so it is in the
//! command list (F9, Scripts); a plugin is installed. The program has no
//! model of its own: it uses the user's API key.

use nmc_app::AppContext;

use super::CmdResult;
use crate::cli::AiArgs;

#[cfg(not(feature = "ai"))]
pub async fn ai(_context: &AppContext, _args: AiArgs) -> CmdResult {
    Err("`nmc ai` is not part of this build (server builds leave it out)".into())
}

#[cfg(feature = "ai")]
pub async fn ai(context: &AppContext, args: AiArgs) -> CmdResult {
    use crate::cli::AiAction;
    match args.action {
        AiAction::Key => engine::save_key(context),
        AiAction::New { description, plugin, out, fixture, tries, model, force } => {
            let request = engine::Request {
                description: description.join(" "),
                plugin,
                out,
                fixture,
                tries: tries.max(1),
                model,
                force,
            };
            engine::new(context, request).await
        }
        AiAction::Rename { description, paths, model, json } => {
            engine::rename(context, &description, &paths, &model, json).await
        }
    }
}

#[cfg(feature = "ai")]
mod engine {
    use std::fs;
    use std::io::Read;
    use std::path::{Component, Path, PathBuf};
    use std::process::ExitCode;
    use std::time::Duration;

    use nmc_app::AppContext;
    use serde::Deserialize;
    use serde_json::{Value, json};

    use crate::cli::TestArgs;
    use crate::commands::CmdResult;
    use crate::commands::script_test;

    /// The API, unless `NMC_AI_URL` names another server (tests).
    const API: &str = "https://api.anthropic.com";
    /// The file `nmc ai key` writes, in the settings folder.
    const KEY_FILE: &str = "anthropic-api-key";
    /// Models that fall back to another model when they decline a request
    /// (`fallbacks: "default"`).
    const WITH_FALLBACKS: [&str; 5] = [
        "claude-opus-5-5",
        "claude-opus-5",
        "claude-sonnet-5-5",
        "claude-fable-5-2",
        "claude-fable-5-1",
    ];

    pub struct Request {
        pub description: String,
        pub plugin: bool,
        pub out: Option<String>,
        pub fixture: Option<String>,
        pub tries: u32,
        pub model: String,
        pub force: bool,
    }

    /// What Claude answers each time (structured output).
    #[derive(Deserialize)]
    struct Answer {
        summary: String,
        files: Vec<AnswerFile>,
    }

    #[derive(Deserialize)]
    struct AnswerFile {
        path: String,
        content: String,
    }

    /// `nmc ai key`: saves the key read from standard input, readable only
    /// by the user.
    pub fn save_key(context: &AppContext) -> CmdResult {
        let mut key = String::new();
        std::io::stdin().read_to_string(&mut key)?;
        let key = key.trim();
        if key.is_empty() {
            return Err("no key on standard input (echo KEY | nmc ai key)".into());
        }
        let path = context.paths.config_dir.join(KEY_FILE);
        fs::create_dir_all(&context.paths.config_dir)?;
        write_private(&path, key)?;
        crate::out!("saved the key in {}", path.display());
        Ok(ExitCode::SUCCESS)
    }

    fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)?;
            file.write_all(text.as_bytes())
        }
        #[cfg(not(unix))]
        fs::write(path, text)
    }

    /// The key: `ANTHROPIC_API_KEY`, else the one `nmc ai key` saved.
    fn api_key(context: &AppContext) -> Result<String, String> {
        if let Ok(key) = std::env::var("ANTHROPIC_API_KEY")
            && !key.trim().is_empty()
        {
            return Ok(key.trim().to_owned());
        }
        fs::read_to_string(context.paths.config_dir.join(KEY_FILE))
            .map(|key| key.trim().to_owned())
            .ok()
            .filter(|key| !key.is_empty())
            .ok_or_else(|| {
                "no Anthropic API key: set ANTHROPIC_API_KEY or save one with `nmc ai key`".into()
            })
    }

    /// `nmc ai new`: ask, test, fix, save.
    pub async fn new(context: &AppContext, request: Request) -> CmdResult {
        let key = api_key(context)?;
        let client = Client::new(key, request.model.clone())?;
        let draft = tempfile::Builder::new().prefix("nmc-ai-").tempdir()?;
        let mut messages = vec![json!({ "role": "user", "content": first_message(&request) })];
        for attempt in 1..=request.tries {
            crate::out!("asking {} (try {attempt} of {})…", request.model, request.tries);
            let content = client.ask(&system_prompt(), &messages, answer_schema()).await?;
            let answer: Answer = parse_answer(&content)?;
            // The assistant turn goes back unchanged (thinking included).
            messages.push(json!({ "role": "assistant", "content": content }));
            let folder = draft.path().join(format!("try-{attempt}"));
            let target = match write_answer(&answer, &folder, request.plugin) {
                Ok(target) => target,
                Err(problem) => {
                    messages.push(json!({ "role": "user", "content": problem }));
                    continue;
                }
            };
            let test = TestArgs {
                target: target.display().to_string(),
                args: Vec::new(),
                fixture: request.fixture.clone(),
                command: Vec::new(),
                select: Vec::new(),
                timeout: 30.0,
                json: true,
                keep: false,
            };
            let report = script_test::report(context, &test).await?;
            if report.passed {
                let saved = save(context, &request, &target)?;
                crate::out!("{}", answer.summary.trim());
                for line in script_test::summary(&report) {
                    crate::out!("{line}");
                }
                crate::out!("saved {}", saved);
                crate::out!(
                    "It passed on sample files; read it before using it on files that matter."
                );
                return Ok(ExitCode::SUCCESS);
            }
            crate::out!("the test failed; asking for a fix");
            messages.push(json!({
                "role": "user",
                "content": format!(
                    "`nmc test` did not pass. Fix the files and answer with all of them again.\n\n{}",
                    serde_json::to_string_pretty(&report)?
                ),
            }));
        }
        let kept = draft.keep();
        Err(format!(
            "no version passed `nmc test` in {} tries; the drafts are in {}",
            request.tries,
            kept.display()
        )
        .into())
    }

    fn first_message(request: &Request) -> String {
        let kind = if request.plugin { "a plugin" } else { "a script" };
        format!("Write {kind} that does this:\n\n{}", request.description)
    }

    /// Everything Claude needs: the guide, the API and the answer format.
    fn system_prompt() -> String {
        format!(
            "You write TypeScript scripts and plugins for NemesisCommander, a dual-pane file \
             manager. Your answer is tested with `nmc test` on sample files; when the test \
             fails you get its report and answer again with every file.\n\n\
             Answer with `summary` (one or two sentences for the user on what it does and any \
             limits) and `files`:\n\
             - for a script: one file, a short kebab-case name ending in .ts \
             (e.g. `sort-photos.ts`). It runs from the command list on the active panel: read \
             `nmc.context` (panel folder, other folder, selection) when it is not null, and \
             fall back to `nmc.args[0]` as the folder otherwise.\n\
             - for a plugin: `plugin.json` and its script (`main.ts`); ask in the manifest \
             only for `$PANEL`, `$OTHER` or `$SELECTION`.\n\
             Paths are relative, without folders other than the plugin's own. Use only the \
             `nmc` API below and standard JavaScript. Print a short line about what was done.\n\n\
             <guide>\n{}\n</guide>\n\n<api file=\"nmc.d.ts\">\n{}\n</api>",
            nmc_script::GUIDE,
            nmc_script::TYPES
        )
    }

    fn answer_schema() -> Value {
        json!({
            "type": "object",
            "properties": {
                "summary": { "type": "string" },
                "files": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": { "type": "string" },
                            "content": { "type": "string" }
                        },
                        "required": ["path", "content"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["summary", "files"],
            "additionalProperties": false
        })
    }

    /// The answer in the text blocks of `content`.
    fn parse_answer<T: serde::de::DeserializeOwned>(content: &Value) -> Result<T, String> {
        let text: String = content
            .as_array()
            .into_iter()
            .flatten()
            .filter(|block| block["type"] == "text")
            .filter_map(|block| block["text"].as_str())
            .collect();
        serde_json::from_str(&text).map_err(|e| format!("Claude's answer is not readable: {e}"))
    }

    /// Writes the answer's files into `folder`; returns what to test (the
    /// script, or the plugin folder), or the problem to tell Claude.
    fn write_answer(answer: &Answer, folder: &Path, plugin: bool) -> Result<PathBuf, String> {
        if answer.files.is_empty() || answer.files.len() > 10 {
            return Err(format!("Answer with 1 to 10 files, not {}.", answer.files.len()));
        }
        for file in &answer.files {
            let path = Path::new(&file.path);
            let inside = !file.path.is_empty()
                && path.components().all(|c| matches!(c, Component::Normal(_)));
            if !inside {
                return Err(format!("{:?}: use a relative path inside the plugin.", file.path));
            }
            let destination = folder.join(path);
            let written = fs::create_dir_all(destination.parent().unwrap_or(folder))
                .and_then(|()| fs::write(&destination, &file.content));
            written.map_err(|e| format!("cannot write {}: {e}", file.path))?;
        }
        if plugin {
            if !folder.join("plugin.json").is_file() {
                return Err("A plugin needs a plugin.json.".into());
            }
            return Ok(folder.to_path_buf());
        }
        let scripts: Vec<&AnswerFile> = answer
            .files
            .iter()
            .filter(|f| [".ts", ".js"].iter().any(|ext| f.path.ends_with(ext)))
            .collect();
        match scripts.as_slice() {
            [script] if !script.path.contains('/') => Ok(folder.join(&script.path)),
            _ => Err("A script is one .ts file, without folders.".into()),
        }
    }

    /// Saves what passed: `--out`, else the scripts folder (a script) or
    /// the installed plugins (a plugin). Returns where.
    fn save(context: &AppContext, request: &Request, target: &Path) -> Result<String, String> {
        let error = |e: std::io::Error| e.to_string();
        if request.plugin {
            if let Some(out) = &request.out {
                let out = Path::new(out);
                refuse_existing(out, request.force)?;
                copy_folder(target, out).map_err(error)?;
                return Ok(format!("{} (install it with `nmc plugin install`)", out.display()));
            }
            let plugin = nmc_script::plugin::Plugin::load(target).map_err(|e| e.to_string())?;
            let installed = context.paths.plugins_dir().join(plugin.name());
            refuse_existing(&installed, request.force)?;
            nmc_script::plugin::install(target, &context.paths.plugins_dir())
                .map_err(|e| e.to_string())?;
            return Ok(format!(
                "and installed plugin {} (F9, or Alt+X; it asks for its permissions at first use)",
                plugin.name()
            ));
        }
        let name = target.file_name().unwrap_or_default();
        let destination = match &request.out {
            Some(out) => PathBuf::from(out),
            None => {
                fs::create_dir_all(context.paths.scripts_dir()).map_err(error)?;
                context.paths.scripts_dir().join(name)
            }
        };
        refuse_existing(&destination, request.force)?;
        fs::copy(target, &destination).map_err(error)?;
        let place = if request.out.is_none() { " (F9, Scripts)" } else { "" };
        Ok(format!("{}{place}", destination.display()))
    }

    fn refuse_existing(path: &Path, force: bool) -> Result<(), String> {
        if path.exists() && !force {
            return Err(format!("{} exists; use --out or --force", path.display()));
        }
        Ok(())
    }

    fn copy_folder(from: &Path, to: &Path) -> std::io::Result<()> {
        fs::create_dir_all(to)?;
        for entry in fs::read_dir(from)? {
            let entry = entry?;
            let target = to.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_folder(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        Ok(())
    }

    /// What Claude answers for a rename: a multi-rename rule.
    #[derive(Deserialize)]
    struct RenameAnswer {
        explanation: String,
        template: String,
        find: String,
        replace: String,
        regex: bool,
        case: nmc_ops::CaseChange,
        counter_start: u64,
        counter_step: u64,
        counter_width: usize,
    }

    fn rename_schema() -> Value {
        let text = json!({ "type": "string" });
        let number = json!({ "type": "integer" });
        json!({
            "type": "object",
            "properties": {
                "explanation": text, "template": text, "find": text, "replace": text,
                "regex": { "type": "boolean" },
                "case": { "type": "string", "enum": ["Unchanged", "Lower", "Upper", "Title"] },
                "counter_start": number, "counter_step": number, "counter_width": number
            },
            "required": ["explanation", "template", "find", "replace", "regex", "case",
                "counter_start", "counter_step", "counter_width"],
            "additionalProperties": false
        })
    }

    /// The names Claude sees: enough to show the pattern.
    const RENAME_SAMPLES: usize = 40;

    /// `nmc ai rename`: Claude writes the multi-rename rule (template,
    /// find/replace, regex, case, counter) for the description; the rule
    /// is checked on the files and fixed when it fails. Prints the rule
    /// and the new names; renames nothing.
    pub async fn rename(
        context: &AppContext,
        description: &str,
        paths: &[String],
        model: &str,
        as_json: bool,
    ) -> CmdResult {
        let key = api_key(context)?;
        let client = Client::new(key, model.to_owned())?;
        let files = crate::location::parse_all(paths)?;
        let names: Vec<&str> =
            files.iter().map(|path| path.file_name().unwrap_or_default()).collect();
        let shown = names.iter().take(RENAME_SAMPLES).copied().collect::<Vec<_>>().join("\n");
        let more = names.len().saturating_sub(RENAME_SAMPLES);
        let mut messages = vec![json!({
            "role": "user",
            "content": format!(
                "Rename these {} files: {description}\n\n<names>\n{shown}\n</names>{}",
                names.len(),
                if more > 0 { format!("\n(and {more} more like them)") } else { String::new() }
            ),
        })];
        for _ in 0..3 {
            let content = client.ask(RENAME_PROMPT, &messages, rename_schema()).await?;
            let answer: RenameAnswer = parse_answer(&content)?;
            messages.push(json!({ "role": "assistant", "content": content }));
            let rule = nmc_ops::RenameRule {
                template: answer.template,
                find: answer.find,
                replace: answer.replace,
                regex: answer.regex,
                case: answer.case,
                counter_start: answer.counter_start,
                counter_step: answer.counter_step.max(1),
                counter_width: answer.counter_width.max(1),
            };
            match nmc_ops::preview_renames(&files, &rule) {
                Ok(new_names) => {
                    // Names that belong to other files: `nmc rename` and
                    // the window refuse them, so the preview shows them.
                    let conflicts =
                        nmc_ops::rename_conflicts(&context.registry, &files, &new_names).await?;
                    let taken: Vec<&str> =
                        conflicts.iter().filter_map(|path| path.file_name()).collect();
                    if as_json {
                        let report = json!({
                            "explanation": answer.explanation,
                            "rule": rule,
                            "names": new_names,
                            "conflicts": taken,
                        });
                        crate::out!("{report}");
                    } else {
                        crate::out!("{}", answer.explanation.trim());
                        crate::out!("{}", rename_command(&rule));
                        for ((path, old), new) in files.iter().zip(&names).zip(&new_names) {
                            if old != new {
                                let mark = if conflicts.contains(&nmc_ops::renamed(path, new)) {
                                    "  (already exists)"
                                } else {
                                    ""
                                };
                                crate::out!("{old} -> {new}{mark}");
                            }
                        }
                    }
                    return Ok(ExitCode::SUCCESS);
                }
                Err(error) => messages.push(json!({
                    "role": "user",
                    "content": format!("This rule fails on these files: {error}. Fix it."),
                })),
            }
        }
        Err("Claude found no working rule; try describing it differently".into())
    }

    /// The `nmc rename` options of `rule`, to see (and reuse) it.
    fn rename_command(rule: &nmc_ops::RenameRule) -> String {
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
        let mut line = format!("nmc rename --template {}", quote(&rule.template));
        if !rule.find.is_empty() {
            line += &format!(" --find {} --replace {}", quote(&rule.find), quote(&rule.replace));
            if rule.regex {
                line += " --regex";
            }
        }
        let case = match rule.case {
            nmc_ops::CaseChange::Unchanged => "",
            nmc_ops::CaseChange::Lower => " --case lower",
            nmc_ops::CaseChange::Upper => " --case upper",
            nmc_ops::CaseChange::Title => " --case title",
        };
        line += case;
        if rule.template.contains("[C]") {
            line += &format!(
                " --start {} --step {} --digits {}",
                rule.counter_start, rule.counter_step, rule.counter_width
            );
        }
        line
    }

    const RENAME_PROMPT: &str = "You write rules for the multi-rename of NemesisCommander, a \
        file manager. A rule renames every file the same way, in this order:\n\
        1. `template` builds the name: [N] name without extension, [N2-5] characters 2 to 5 \
        of it (1-based), [N3-] from character 3, [E] extension without the dot, [X] \
        extension with the dot (or nothing), [C] counter, [P] parent folder name, [[ and ]] \
        literal brackets; other text is kept. `[N][X]` keeps the name.\n\
        2. `find` / `replace` (when `find` is not empty) on that result: literal text, or \
        with `regex` true a Rust regular expression (no look-around, no backreferences in \
        the pattern) whose `replace` may use $1, ${name}; every match is replaced.\n\
        3. `case`: Unchanged, Lower, Upper or Title.\n\
        The counter starts at `counter_start`, adds `counter_step` per file (in the order \
        given) and has at least `counter_width` digits; use 1, 1, 1 when there is no [C].\n\
        Prefer the simplest rule; keep extensions unless asked. New names must differ from \
        each other. `explanation`: one short sentence, in the language of the request.";

    /// The Messages API over HTTPS (there is no official Rust SDK).
    struct Client {
        http: reqwest::Client,
        key: String,
        model: String,
        url: String,
    }

    impl Client {
        fn new(key: String, model: String) -> Result<Self, String> {
            // reqwest is built without a crypto provider; use ring, as
            // WebDAV and S3 do. Installing twice fails harmlessly.
            let _ = rustls::crypto::ring::default_provider().install_default();
            let http = reqwest::Client::builder()
                .user_agent(concat!("NemesisCommander/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(Duration::from_secs(30))
                .timeout(Duration::from_secs(600))
                .build()
                .map_err(|e| format!("cannot start the HTTP client: {e}"))?;
            let base = std::env::var("NMC_AI_URL").unwrap_or_else(|_| API.to_owned());
            let url = format!("{}/v1/messages", base.trim_end_matches('/'));
            Ok(Client { http, key, model, url })
        }

        /// One request; the answer's content blocks.
        async fn ask(
            &self,
            system: &str,
            messages: &[Value],
            schema: Value,
        ) -> Result<Value, String> {
            let mut body = json!({
                "model": self.model,
                "max_tokens": 16000,
                "system": system,
                "messages": messages,
                "output_config": {
                    "effort": "high",
                    "format": { "type": "json_schema", "schema": schema }
                },
            });
            let mut request = self
                .http
                .post(&self.url)
                .header("x-api-key", &self.key)
                .header("anthropic-version", "2026-10-21")
                .header("content-type", "application/json");
            if WITH_FALLBACKS.contains(&self.model.as_str()) {
                body["fallbacks"] = json!("default");
                request = request.header("anthropic-beta", "server-side-fallback-2026-07-01");
            }
            let response = request
                .body(body.to_string())
                .send()
                .await
                .map_err(|e| format!("cannot reach the Claude API: {e}"))?;
            let status = response.status();
            let text = response.text().await.map_err(|e| format!("the Claude API stopped: {e}"))?;
            let reply: Value = serde_json::from_str(&text)
                .map_err(|_| format!("the Claude API answered {status}: {text}"))?;
            if !status.is_success() {
                let message = reply["error"]["message"].as_str().unwrap_or(&text);
                return Err(format!("the Claude API answered {status}: {message}"));
            }
            match reply["stop_reason"].as_str() {
                Some("refusal") => {
                    let why = reply["stop_details"]["explanation"].as_str().unwrap_or("");
                    Err(format!("Claude declined to write this script. {why}").trim().to_owned())
                }
                Some("max_tokens") => {
                    Err("the answer was too long; ask for something smaller".into())
                }
                _ => Ok(reply["content"].clone()),
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn answer(files: &[(&str, &str)]) -> Answer {
            Answer {
                summary: String::new(),
                files: files
                    .iter()
                    .map(|(p, c)| AnswerFile { path: (*p).into(), content: (*c).into() })
                    .collect(),
            }
        }

        #[test]
        fn answers_stay_in_their_folder() {
            let dir = tempfile::TempDir::new().unwrap();
            let script = write_answer(&answer(&[("a.ts", "x")]), dir.path(), false).unwrap();
            assert_eq!(script, dir.path().join("a.ts"));
            for bad in ["../a.ts", "/etc/a.ts", "sub/a.ts", ""] {
                assert!(write_answer(&answer(&[(bad, "x")]), dir.path(), false).is_err(), "{bad}");
            }
            assert!(write_answer(&answer(&[("main.ts", "x")]), dir.path(), true).is_err());
        }

        #[test]
        fn the_answer_is_read_from_the_text_blocks() {
            let content = json!([
                { "type": "thinking", "thinking": "" },
                { "type": "text", "text": "{\"summary\":\"s\",\"files\":[]}" }
            ]);
            assert_eq!(parse_answer::<Answer>(&content).unwrap().summary, "s");
            assert!(parse_answer::<Answer>(&json!([])).is_err());
        }
    }
}
