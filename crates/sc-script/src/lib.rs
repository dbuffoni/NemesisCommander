//! # sc-script
//!
//! TypeScript scripts for SuperCommander (ADR 0015): `sc run script.ts`.
//!
//! A script is one `.ts` (or `.js`) file. It is transpiled to JavaScript
//! with `deno_ast` and runs as an ES module (top-level `await` works) on V8
//! through `deno_core`: no Node, nothing to install. The script sees one
//! global, `sc` (types in `js/sc.d.ts`, printed by `sc run --types`), whose
//! functions are Rust operations over the same providers and copy engine
//! as the panels. TypeScript orchestrates, Rust moves the bytes.
//!
//! Scripts start with no access: the user grants locations ([`Permissions`]).
//! The V8 heap is limited ([`ScriptOptions::heap_limit`]), and a script
//! cannot import other modules.

mod ops;
pub mod permissions;
pub mod plugin;

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use deno_core::{JsRuntime, ModuleSpecifier, RuntimeOptions, v8};
use sc_vfs::ProviderRegistry;

pub use ops::{Invocation, Output, READ_TEXT_LIMIT};
pub use permissions::{Access, Permissions};

/// The TypeScript declarations of the `sc` API, for editors.
pub const TYPES: &str = include_str!("../js/sc.d.ts");

/// The JavaScript that defines the `sc` global.
const SDK: &str = include_str!("../js/sdk.js");

/// Default limit of the V8 heap.
pub const DEFAULT_HEAP_LIMIT: usize = 512 * 1024 * 1024;

/// Why a script did not complete.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The source is not valid TypeScript or JavaScript.
    #[error("{0}")]
    Syntax(String),
    /// The script threw (or a promise it awaited rejected).
    #[error("{0}")]
    Failed(String),
    /// The script used more memory than allowed and was stopped.
    #[error("the script used more than {} MiB of memory and was stopped", .0 / (1024 * 1024))]
    OutOfMemory(usize),
    /// The script ran longer than allowed and was stopped.
    #[error("the script ran longer than {} s and was stopped", .0.as_secs_f64())]
    TimedOut(Duration),
}

/// How to run a script.
pub struct ScriptOptions {
    /// Providers for every location the script names.
    pub registry: Arc<ProviderRegistry>,
    /// The folder relative paths start from.
    pub base: PathBuf,
    /// What the script may touch.
    pub permissions: Permissions,
    /// `sc.args`.
    pub args: Vec<String>,
    /// Receives `sc.print` output, one call per print.
    pub output: Output,
    /// Limit of the V8 heap in bytes.
    pub heap_limit: usize,
    /// How long the script may run; `None` for no limit.
    pub time_limit: Option<Duration>,
    /// For a plugin command: where it was started (`sc.context`).
    pub invocation: Option<Invocation>,
}

/// Transpiles `source` to JavaScript when `name` ends in `.ts`, `.mts` or
/// `.tsx`; other sources are returned as they are.
pub fn transpile(name: &str, source: &str) -> Result<String, ScriptError> {
    let media_type = if [".ts", ".mts"].iter().any(|ext| name.ends_with(ext)) {
        deno_ast::MediaType::TypeScript
    } else if name.ends_with(".tsx") {
        deno_ast::MediaType::Tsx
    } else {
        return Ok(source.to_owned());
    };
    let parsed = deno_ast::parse_module(deno_ast::ParseParams {
        specifier: specifier(name),
        text: source.into(),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|e| ScriptError::Syntax(e.to_string()))?;
    let transpiled = parsed
        .transpile(&Default::default(), &Default::default(), &Default::default())
        .map_err(|e| ScriptError::Syntax(e.to_string()))?;
    Ok(transpiled.into_source().text)
}

/// Runs the script `source` (named `name` in messages) to completion,
/// including everything it awaits.
///
/// V8 needs a thread of its own with a single-threaded runtime, so the
/// script runs on a new thread; this future can be awaited from anywhere.
pub async fn run(name: &str, source: &str, options: ScriptOptions) -> Result<(), ScriptError> {
    let (name, source) = (name.to_owned(), source.to_owned());
    let (done, result) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("sc-script".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build();
            let outcome = match runtime {
                Ok(runtime) => runtime.block_on(run_here(&name, &source, options)),
                Err(error) => Err(ScriptError::Failed(format!("cannot start the script: {error}"))),
            };
            let _ = done.send(outcome);
        })
        .map_err(|e| ScriptError::Failed(format!("cannot start the script: {e}")))?;
    result.await.unwrap_or_else(|_| Err(ScriptError::Failed("the script stopped abruptly".into())))
}

/// [`run`] on the calling thread, inside a current-thread runtime.
async fn run_here(name: &str, source: &str, options: ScriptOptions) -> Result<(), ScriptError> {
    let code = transpile(name, source)?;
    let (heap_limit, time_limit) = (options.heap_limit, options.time_limit);
    let state = ops::ScriptState {
        registry: options.registry,
        base: options.base,
        permissions: options.permissions,
        args: options.args,
        output: options.output,
        invocation: options.invocation,
    };
    let mut runtime = JsRuntime::new(RuntimeOptions {
        extensions: vec![ops::sc_script::init(state)],
        create_params: Some(v8::CreateParams::default().heap_limits(0, heap_limit)),
        ..Default::default()
    });

    // Near the limit, stop the script instead of letting V8 abort the
    // process; give V8 room to unwind.
    let out_of_memory = Arc::new(AtomicBool::new(false));
    let isolate = runtime.v8_isolate().thread_safe_handle();
    let flag = out_of_memory.clone();
    runtime.add_near_heap_limit_callback(move |current, _initial| {
        flag.store(true, Ordering::SeqCst);
        isolate.terminate_execution();
        current * 2
    });
    // A busy loop never yields to the runtime: a watchdog thread stops it.
    let timed_out = Arc::new(AtomicBool::new(false));
    let _watchdog = time_limit.map(|limit| {
        Watchdog::start(limit, runtime.v8_isolate().thread_safe_handle(), timed_out.clone())
    });
    let failed = |error: deno_core::error::CoreError| {
        if out_of_memory.load(Ordering::SeqCst) {
            ScriptError::OutOfMemory(heap_limit)
        } else if let Some(limit) = time_limit.filter(|_| timed_out.load(Ordering::SeqCst)) {
            ScriptError::TimedOut(limit)
        } else {
            ScriptError::Failed(error.to_string())
        }
    };

    let script = async {
        runtime.execute_script("ext:sc/sdk.js", SDK).map_err(|e| failed(e.into()))?;
        let module =
            runtime.load_main_es_module_from_code(&specifier(name), code).await.map_err(failed)?;
        let evaluated = runtime.mod_evaluate(module);
        runtime.run_event_loop(Default::default()).await.map_err(failed)?;
        evaluated.await.map_err(failed)
    };
    // Awaiting a long operation is not JavaScript running: the time limit
    // also applies to the whole run.
    match time_limit {
        Some(limit) => {
            tokio::time::timeout(limit, script).await.unwrap_or(Err(ScriptError::TimedOut(limit)))
        }
        None => script.await,
    }
}

/// Stops a script's JavaScript when its time is up; dropped when the script
/// ends first.
struct Watchdog {
    _finished: std::sync::mpsc::Sender<()>,
}

impl Watchdog {
    fn start(limit: Duration, isolate: v8::IsolateHandle, timed_out: Arc<AtomicBool>) -> Self {
        let (finished, wait) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            // Disconnected (the script ended) wakes the thread early.
            if wait.recv_timeout(limit) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                timed_out.store(true, Ordering::SeqCst);
                isolate.terminate_execution();
            }
        });
        Watchdog { _finished: finished }
    }
}

/// The module name of a script in messages and stack traces.
fn specifier(name: &str) -> ModuleSpecifier {
    let path = std::path::Path::new(name);
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    // The runtime compares normalized URLs: `a/../b` must already be `b`.
    let absolute = std::fs::canonicalize(&absolute).unwrap_or(absolute);
    // Windows canonical paths start with `\\?\`, which URLs cannot hold.
    #[cfg(windows)]
    let absolute = absolute
        .to_str()
        .and_then(|text| text.strip_prefix(r"\\?\"))
        .map(PathBuf::from)
        .unwrap_or(absolute);
    ModuleSpecifier::from_file_path(&absolute)
        .unwrap_or_else(|_| ModuleSpecifier::parse("file:///script.ts").expect("a valid URL"))
}
