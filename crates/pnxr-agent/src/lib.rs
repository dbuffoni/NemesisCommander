//! # pnxr-agent
//!
//! Pairnox's remote agent, in both directions:
//!
//! * [`server`]: what `pnxr agent --stdio` runs on a Linux server. It serves
//!   the local file system through the protocol of `pnxr-proto`, within the
//!   [`Limits`] given on its command line (allowed roots, read-only).
//! * [`client`]: [`AgentProvider`], the `agent://user@host/path` provider
//!   used by the GUI, the TUI and the CLI.
//! * [`launch`]: starts the agent over SSH and installs `pnxr` in
//!   `~/.cache/pnxr/` when the server does not have it.
//! * [`serve`]: `pnxr serve`, the permanent agent over mutual TLS, and the
//!   `scs://host:port/path` provider that connects to it.
//! * [`provision`]: `pnxr serve install HOST`, which sets up `pnxr serve` on a
//!   server over SSH (program, certificates, systemd unit).
//! * [`route`]: the controller's side of server-to-server copies.
//! * `transfer`: server-to-server copies run by `pnxr serve`.

pub mod client;
pub mod launch;
pub mod provision;
pub mod route;
pub mod serve;
pub mod server;
mod transfer;

pub use client::AgentProvider;
pub use launch::{AgentFactory, connect_agent};
pub use serve::{ServeFactory, host_and_port};
pub use server::{AgentService, Limits, ServeState, serve_stdio};

/// Version of this program; the controller and the agent must match.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
