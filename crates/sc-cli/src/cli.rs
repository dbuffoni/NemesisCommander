//! Command-line syntax (clap definitions).

use clap::{Args, Parser, Subcommand, ValueEnum};

/// SuperCommander: a fast dual-pane file manager for local and remote files.
///
/// Run without a command to start the terminal UI. Locations can be local
/// paths or URIs such as sftp://user@host/dir, smb://host/share/dir,
/// davs://host/dir, s3://bucket/dir,
/// agent://host/dir, scs://host:port/dir (sc serve) or
/// zip:///path/archive.zip!/inside.
#[derive(Debug, Parser)]
#[command(name = "sc", version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Locations for the left and right panel of the terminal UI.
    #[arg(value_name = "LOCATION", num_args = 0..=2)]
    pub locations: Vec<String>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List a directory.
    Ls(LsArgs),
    /// Copy files and directories.
    Cp(TransferArgs),
    /// Move or rename files and directories.
    Mv(TransferArgs),
    /// Delete files and directories.
    Rm(RmArgs),
    /// Create directories.
    Mkdir(MkdirArgs),
    /// Print a file to standard output.
    Cat(CatArgs),
    /// Print checksums.
    Hash(HashArgs),
    /// Search by name, size, date and content.
    Find(FindArgs),
    /// Create an archive.
    Pack(PackArgs),
    /// Extract an archive.
    Unpack(UnpackArgs),
    /// Rename many files with a template.
    Rename(RenameArgs),
    /// Show and control the background transfer queue.
    Jobs(JobsArgs),
    /// Export, import and locate settings.
    Config(ConfigArgs),
    /// Print the main keyboard shortcuts of the user interface.
    Keys,
    /// Serve a remote controller over standard input/output (Linux only).
    Agent(AgentArgs),
    /// Serve this machine permanently over mutual TLS (Linux only).
    Serve(ServeArgs),
    /// Certificates for `sc serve`: create the authority, issue server
    /// bundles.
    Tls(TlsArgs),
    /// Run a TypeScript (or JavaScript) script with the `sc` API.
    Run(RunArgs),
    /// Install, list, allow and run TypeScript plugins.
    Plugin(PluginArgs),
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SortArg {
    Name,
    Ext,
    Size,
    Date,
}

#[derive(Debug, Args)]
pub struct LsArgs {
    /// Directory to list (default: current directory).
    pub path: Option<String>,
    /// Long format with permissions, size and date.
    #[arg(short, long)]
    pub long: bool,
    /// Include hidden (dot) files.
    #[arg(short, long)]
    pub all: bool,
    /// Sort column.
    #[arg(long, value_enum, default_value_t = SortArg::Name)]
    pub sort: SortArg,
    /// Reverse the order.
    #[arg(short, long)]
    pub reverse: bool,
    /// Exact sizes in bytes instead of KiB/MiB.
    #[arg(long)]
    pub bytes: bool,
    /// One JSON object per entry.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum ConflictArg {
    /// Ask on the terminal (fails when not interactive).
    Ask,
    Overwrite,
    /// Overwrite only if the source is newer.
    Newer,
    Skip,
    /// Keep both, renaming the new file.
    Rename,
    Fail,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum VerifyArg {
    None,
    Size,
    /// XXH3 content hash.
    Fast,
    /// BLAKE3 content hash.
    Full,
}

#[derive(Debug, Args)]
pub struct TransferArgs {
    /// Sources followed by the destination.
    #[arg(required = true, num_args = 2..)]
    pub paths: Vec<String>,
    /// What to do when a destination exists.
    #[arg(long, value_enum)]
    pub conflict: Option<ConflictArg>,
    /// How to verify copied files.
    #[arg(long, value_enum)]
    pub verify: Option<VerifyArg>,
    /// Files copied in parallel.
    #[arg(short = 'j', long)]
    pub parallel: Option<usize>,
    /// Do not keep modification times and permissions.
    #[arg(long)]
    pub no_preserve: bool,
    /// Bandwidth limit, e.g. 10M (bytes per second).
    #[arg(long)]
    pub limit: Option<String>,
    /// Add to the background queue and return immediately (run it with
    /// `sc jobs run`).
    #[arg(long)]
    pub queue: bool,
    /// Between two `sc serve` servers (scs://): how the data travels.
    /// `auto` tries direct, then reverse, then through this machine.
    #[arg(long, value_enum, default_value_t = RouteArg::Auto)]
    pub route: RouteArg,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RouteArg {
    /// Direct, then reverse, then through this machine.
    Auto,
    /// The destination server pulls from the source server.
    Direct,
    /// The source server pushes to the destination server.
    Reverse,
    /// Through this machine, like any other copy.
    Controller,
}

#[derive(Debug, Args)]
pub struct RmArgs {
    /// Items to delete.
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// Move to the recycle bin instead of deleting.
    #[arg(long)]
    pub trash: bool,
    /// Do not ask for confirmation (required when not on a terminal).
    #[arg(short, long)]
    pub yes: bool,
}

#[derive(Debug, Args)]
pub struct MkdirArgs {
    /// Directories to create.
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// Create missing parents too.
    #[arg(short, long)]
    pub parents: bool,
}

#[derive(Debug, Args)]
pub struct CatArgs {
    /// Files to print.
    #[arg(required = true)]
    pub paths: Vec<String>,
}

#[derive(Debug, Args)]
pub struct HashArgs {
    /// Files to hash.
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// blake3, xxh3 or sha256.
    #[arg(short, long, default_value = "blake3")]
    pub algorithm: String,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    /// Where to search (default: current directory).
    pub root: Option<String>,
    /// Name mask, e.g. "*.rs;*.toml;!target*".
    #[arg(short, long)]
    pub name: Option<String>,
    /// Text the files must contain.
    #[arg(short, long)]
    pub content: Option<String>,
    /// Treat --content as a regular expression.
    #[arg(long)]
    pub regex: bool,
    /// Case-insensitive content search.
    #[arg(short = 'i', long)]
    pub ignore_case: bool,
    /// Minimum size (e.g. 10k, 5M).
    #[arg(long)]
    pub min_size: Option<String>,
    /// Maximum size.
    #[arg(long)]
    pub max_size: Option<String>,
    /// Modified within the last N days.
    #[arg(long)]
    pub newer_than_days: Option<u64>,
    /// Modified more than N days ago.
    #[arg(long)]
    pub older_than_days: Option<u64>,
    /// Also report matching directories.
    #[arg(long)]
    pub dirs: bool,
    /// Maximum depth.
    #[arg(long)]
    pub max_depth: Option<usize>,
    /// One JSON object per hit.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Args)]
pub struct PackArgs {
    /// Files and directories to pack.
    #[arg(required = true)]
    pub sources: Vec<String>,
    /// Archive to create; the format follows its extension.
    #[arg(short, long)]
    pub output: String,
    /// Force a format (zip, 7z, tar, tar.gz, tar.xz, tar.bz2, tar.zst, gz, ...).
    #[arg(short, long)]
    pub format: Option<String>,
    /// Compression level 1 (fast) to 9 (small).
    #[arg(short, long, default_value_t = 6)]
    pub level: u32,
}

#[derive(Debug, Args)]
pub struct UnpackArgs {
    /// Archive file.
    pub archive: String,
    /// Destination directory (default: current directory).
    pub destination: Option<String>,
    /// What to do when a file exists.
    #[arg(long, value_enum)]
    pub conflict: Option<ConflictArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum CaseArg {
    Keep,
    Lower,
    Upper,
    Title,
}

#[derive(Debug, Args)]
pub struct RenameArgs {
    /// Files to rename, in counter order.
    #[arg(required = true)]
    pub paths: Vec<String>,
    /// Name template: [N] name, [E] extension, [X] .extension, [C] counter,
    /// [P] parent folder, [N2-5] characters 2 to 5.
    #[arg(short, long, default_value = "[N][X]")]
    pub template: String,
    /// Text to find in the new name.
    #[arg(long, default_value = "")]
    pub find: String,
    /// Replacement for --find.
    #[arg(long, default_value = "")]
    pub replace: String,
    /// --find is a regular expression ($1 groups allowed in --replace).
    #[arg(long)]
    pub regex: bool,
    /// Letter case of the result.
    #[arg(long, value_enum, default_value_t = CaseArg::Keep)]
    pub case: CaseArg,
    /// First counter value.
    #[arg(long, default_value_t = 1)]
    pub start: u64,
    /// Counter step.
    #[arg(long, default_value_t = 1)]
    pub step: u64,
    /// Counter digits (zero padded).
    #[arg(long, default_value_t = 1)]
    pub digits: usize,
    /// Show the new names without renaming.
    #[arg(short = 'n', long)]
    pub dry_run: bool,
}

#[derive(Debug, Args)]
pub struct JobsArgs {
    #[command(subcommand)]
    pub action: Option<JobsAction>,
    /// JSON output for `list`.
    #[arg(long)]
    pub json: bool,
}

#[derive(Debug, Subcommand)]
pub enum JobsAction {
    /// List jobs (default).
    List,
    /// Run queued jobs in the foreground until the queue is empty.
    Run,
    /// Pause a job.
    Pause { id: i64 },
    /// Resume a paused or failed job.
    Resume { id: i64 },
    /// Cancel a job.
    Cancel { id: i64 },
    /// Remove finished jobs.
    Purge,
}

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: ConfigAction,
}

#[derive(Debug, Subcommand)]
pub enum ConfigAction {
    /// Print where settings and data are stored.
    Path,
    /// Print the effective settings.
    Show,
    /// Write all settings to a file.
    Export { file: String },
    /// Read settings from an exported file.
    Import {
        file: String,
        /// Merge bookmarks, connections and key overrides instead of replacing.
        #[arg(long)]
        merge: bool,
    },
}

#[derive(Debug, Args)]
pub struct AgentArgs {
    /// Speak the agent protocol on stdin/stdout (used through SSH).
    #[arg(long, required = true)]
    pub stdio: bool,
    /// Only serve paths below these roots (repeatable). Default: everything.
    #[arg(long = "root")]
    pub roots: Vec<String>,
    /// Refuse every modification.
    #[arg(long)]
    pub read_only: bool,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// The server's certificate bundle (made by `sc tls server`).
    #[arg(long, value_name = "DIR")]
    pub tls: String,
    /// Address and port to listen on.
    #[arg(long, default_value = "0.0.0.0:7443")]
    pub listen: String,
    /// Only serve paths below these roots (repeatable). Default: everything.
    #[arg(long = "root")]
    pub roots: Vec<String>,
    /// Refuse every modification.
    #[arg(long)]
    pub read_only: bool,
}

#[derive(Debug, Args)]
pub struct TlsArgs {
    #[command(subcommand)]
    pub action: TlsAction,
}

#[derive(Debug, Subcommand)]
pub enum TlsAction {
    /// Create this machine's certificate authority and controller
    /// certificate (once).
    Init,
    /// Issue a certificate bundle for a server, to copy there and pass to
    /// `sc serve --tls`.
    Server {
        /// Names the server is reached by: host names or IP addresses, as
        /// written in scs:// locations.
        #[arg(required = true)]
        names: Vec<String>,
        /// Folder to write the bundle into.
        #[arg(long, value_name = "DIR")]
        out: String,
    },
    /// Print the folder holding this machine's certificates.
    Path,
}

#[derive(Debug, Args)]
#[command(after_help = "Scripts start with no access to files: grant locations with \
    --allow-read and --allow-write (repeatable, any location sc understands), \
    or everything with --allow-all.\n\nExample:\n  sc run tidy.ts --allow-write ~/Downloads -- --dry-run")]
pub struct RunArgs {
    /// The script: a .ts or .js file.
    #[arg(required_unless_present = "types")]
    pub script: Option<String>,
    /// Arguments for the script, as `sc.args`.
    #[arg(last = true)]
    pub args: Vec<String>,
    /// Let the script read below this location (repeatable).
    #[arg(long, value_name = "LOCATION")]
    pub allow_read: Vec<String>,
    /// Let the script read and write below this location (repeatable).
    #[arg(long, value_name = "LOCATION")]
    pub allow_write: Vec<String>,
    /// Let the script read and write everything this user can.
    #[arg(long)]
    pub allow_all: bool,
    /// Limit of the script's memory, in MiB.
    #[arg(long, value_name = "MIB", default_value_t = 512)]
    pub memory: usize,
    /// Stop the script after this many seconds.
    #[arg(long, value_name = "SECONDS")]
    pub timeout: Option<f64>,
    /// Print the TypeScript declarations of the `sc` API (for editors)
    /// and exit.
    #[arg(long)]
    pub types: bool,
}

#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub action: PluginAction,
}

#[derive(Debug, Subcommand)]
pub enum PluginAction {
    /// The installed plugins, their commands and permissions.
    List,
    /// Install (or update) the plugin in a folder.
    Install {
        /// A folder with a plugin.json.
        folder: String,
    },
    /// Uninstall a plugin and forget its permissions.
    Remove {
        /// The plugin's name.
        name: String,
    },
    /// Allow a plugin what its manifest asks for.
    Allow {
        /// The plugin's name.
        name: String,
    },
    /// Withdraw what a plugin was allowed.
    Deny {
        /// The plugin's name.
        name: String,
    },
    /// Run a plugin command as if started from the panels.
    Run {
        /// The plugin's name.
        name: String,
        /// The command's id.
        command: String,
        /// The selected entries.
        selection: Vec<String>,
        /// The active panel's folder. Default: the current folder.
        #[arg(long, value_name = "LOCATION")]
        panel: Option<String>,
        /// The other panel's folder.
        #[arg(long, value_name = "LOCATION")]
        other: Option<String>,
    },
}
