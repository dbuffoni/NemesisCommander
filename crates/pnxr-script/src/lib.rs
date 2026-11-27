//! # pnxr-script
//!
//! TypeScript scripts for Pairnox (ADR 0015): `pnxr run script.ts`.
//!
//! A script is one `.ts` (or `.js`) file. It is transpiled to JavaScript
//! with `deno_ast` and runs as an ES module (top-level `await` works) on V8
//! through `deno_core`: no Node, nothing to install. The script sees one
//! global, `pnxr` (types in `js/pnxr.d.ts`, printed by `pnxr run --types`), whose
//! functions are Rust operations over the same providers and copy engine
//! as the panels. TypeScript orchestrates, Rust moves the bytes.
//!
//! Scripts start with no access: the user grants locations ([`Permissions`]).
//! The V8 heap is limited ([`ScriptOptions::heap_limit`]), and a script
//! cannot import other modules.

//!
//! Without the `engine` feature (on by default) the crate has no V8: only
//! plugin manifests, grants and permissions, for an interface that runs
//! scripts through the `pnxr` program (the desktop window does).

pub mod catalog;
#[cfg(feature = "engine")]
mod engine;
#[cfg(feature = "engine")]
mod hashcache;
#[cfg(feature = "engine")]
mod imagehash;
#[cfg(feature = "engine")]
mod ops;
pub mod permissions;
pub mod plugin;
pub mod source;

use std::time::Duration;

#[cfg(feature = "engine")]
pub use engine::{ScriptOptions, run, transpile};
#[cfg(feature = "engine")]
pub use ops::{Invocation, Output, READ_TEXT_LIMIT};
pub use permissions::{Access, Permissions};

/// The TypeScript declarations of the `pnxr` API, for editors.
pub const TYPES: &str = include_str!("../js/pnxr.d.ts");

/// How to write and test scripts and plugins, for people and AI
/// assistants (`pnxr run --guide`).
pub const GUIDE: &str = include_str!("../js/GUIDE.md");

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
