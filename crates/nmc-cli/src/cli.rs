//! Command-line syntax (clap definitions).

use clap::{Args, Parser, Subcommand, ValueEnum};

/// NemesisCommander: a fast dual-pane file manager for local and remote files.
///
/// Run without a command to start the terminal UI. Locations can be local
/// paths or URIs such as sftp://user@host/dir, smb://host/share/dir,
/// davs://host/dir, s3://bucket/dir,
/// agent://host/dir, scs://host:port/dir (nmc serve) or
/// zip:///path/archive.zip!/inside.
#[derive(Debug, Parser)]
#[command(name = "nmc", version, args_conflicts_with_subcommands = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    /// Locations for the left and right panel of the terminal UI.
    #[arg(value_name = "LOCATION", num_args = 0..=2)]
    pub locations: Vec<String>,
    /// On exit, write the folder of the active panel to this file (the shell
    /// function of `nmc shell-init` uses it to change folder).
    #[arg(long, value_name = "FILE")]
    pub cwd_file: Option<std::path::PathBuf>,
}

/// The shells `nmc shell-init` knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Pwsh,
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
    /// Rename many files with a template; --preview shows the new names
    /// without renaming.
    Rename(RenameArgs),
    /// Show and control the background transfer queue.
    Jobs(JobsArgs),
    /// Locate, show, export and import settings, or send them to other
    /// machines.
    Config(ConfigArgs),
    /// Print the main keyboard shortcuts of the user interface.
    Keys,
    /// Serve file operations to an AI assistant over MCP (standard input
    /// and output). Read-only unless folders are granted with --write.
    Mcp(McpArgs),
    /// Print the shell function that makes `nmc` leave the shell in the
    /// folder of the active panel when it ends. Add it to the startup file:
    /// `eval "$(nmc shell-init bash)"` (zsh the same; fish: `nmc shell-init
    /// fish | source`; pwsh: `nmc shell-init pwsh | Out-String | Invoke-Expression`).
    ShellInit {
        /// The shell to print the function for.
        #[arg(value_enum)]
        shell: Shell,
    },
    /// Serve a remote controller over standard input/output (Linux only).
    Agent(AgentArgs),
    /// Serve this machine permanently over mutual TLS (Linux only).
    Serve(ServeArgs),
    /// Certificates for `nmc serve`: create the authority, issue server
    /// bundles.
    Tls(TlsArgs),
    /// Run a TypeScript (or JavaScript) script with the `nmc` API.
    Run(RunArgs),
    /// Install, list, allow and run TypeScript plugins.
    Plugin(PluginArgs),
    /// The launcher and icon of the window on a Linux desktop.
    Desktop(DesktopArgs),
    /// Try a script or a plugin on sample files in a throwaway folder and
    /// report what it did (for authors and AI assistants).
    Test(TestArgs),
    /// Have Claude write a script or a plugin (tested with `nmc test` until
    /// it passes), or a multi-rename rule.
    Ai(AiArgs),
    /// Open the terminal UI as a chooser and print the chosen paths, one per
    /// line (for editors, shell scripts and `$(nmc pick)`).
    Pick(PickArgs),
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
    /// `nmc jobs run`).
    #[arg(long)]
    pub queue: bool,
    /// Between two `nmc serve` servers (scs://): how the data travels.
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
    /// Files (or folders, with --write) to hash.
    #[arg(required_unless_present = "check")]
    pub paths: Vec<String>,
    /// blake3, xxh3 or sha256.
    #[arg(short, long, default_value = "blake3")]
    pub algorithm: String,
    /// Write the checksums of the paths to this file, in the format of
    /// `sha256sum` / `b3sum` (names relative to its folder).
    #[arg(short, long, value_name = "FILE", conflicts_with = "check")]
    pub write: Option<String>,
    /// With --write: also replace the file when it exists.
    #[arg(long, requires = "write")]
    pub force: bool,
    /// Write one `name.sha256` / `name.b3` next to each file instead.
    #[arg(long, conflicts_with_all = ["write", "check"])]
    pub per_file: bool,
    /// Check the files listed in this checksum file, as `sha256sum -c`.
    #[arg(short, long, value_name = "FILE")]
    pub check: Option<String>,
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
    /// Also search inside archive files (zip, tar, 7z, ...).
    #[arg(long)]
    pub archives: bool,
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
#[command(after_help = "Prints each changed name as `old -> new` first. A new name that \
    belongs to another file is marked `(already exists)`, and then nothing is renamed. \
    With --preview (or -n) nothing is renamed at all.\n\nExamples:\n  \
    nmc rename --preview -t 'photo_[C][X]' --digits 3 *.jpg\n  \
    nmc rename --find ' ' --replace _ --case lower *")]
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
    /// Preview: show the new names (and names that already exist) without
    /// renaming.
    #[arg(short = 'n', long, visible_alias = "preview")]
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
    /// Send these settings to other machines: `agent://host` or
    /// `scs://host` (the agent knows where its settings live), or the
    /// settings file or folder on any location (`sftp://host/home/me/.config/nemesiscommander`).
    /// The previous file is kept as config.toml.bak.
    Push {
        /// Machines or settings files to send to.
        #[arg(required = true)]
        locations: Vec<String>,
        /// Keep their bookmarks, connections and shortcuts, adding these.
        #[arg(long)]
        merge: bool,
    },
    /// Replace these settings with another machine's (same locations as
    /// `push`, or a file made by `export`).
    Pull {
        /// Machine or settings file to fetch from.
        location: String,
        /// Keep these bookmarks, connections and shortcuts, adding theirs.
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
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
pub struct ServeArgs {
    #[command(subcommand)]
    pub action: Option<ServeAction>,
    /// The server's certificate bundle (made by `nmc tls server`).
    #[arg(long, value_name = "DIR", required = true)]
    pub tls: Option<String>,
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

#[derive(Debug, Subcommand)]
pub enum ServeAction {
    /// Install and start `nmc serve` on a Linux server over SSH: copies
    /// nmc, issues and copies the server's certificate, and runs it as a
    /// systemd user service.
    Install(ServeInstallArgs),
    /// Stop and remove `nmc serve` (the service and its certificates) from
    /// a server.
    Uninstall {
        /// The server as for SSH: `[user@]host[:ssh-port]`.
        host: String,
    },
}

#[derive(Debug, Args)]
pub struct ServeInstallArgs {
    /// The server as for SSH: `[user@]host[:ssh-port]`.
    pub host: String,
    /// The name in the server's certificate and in `scs://` locations
    /// (default: the host).
    #[arg(long)]
    pub name: Option<String>,
    /// TCP port `nmc serve` listens on.
    #[arg(long, default_value_t = 7443)]
    pub port: u16,
    /// Only serve paths below these folders (repeatable). Default: everything.
    #[arg(long = "root")]
    pub roots: Vec<String>,
    /// Refuse every modification.
    #[arg(long)]
    pub read_only: bool,
    /// Issue a new certificate even if the server has one.
    #[arg(long)]
    pub renew: bool,
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
    /// `nmc serve --tls`.
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
    --allow-read and --allow-write (repeatable, any location nmc understands), \
    or everything with --allow-all.\n\nExample:\n  nmc run tidy.ts --allow-write ~/Downloads -- --dry-run")]
pub struct RunArgs {
    /// The script: a .ts or .js file.
    #[arg(required_unless_present_any = ["types", "guide"])]
    pub script: Option<String>,
    /// Arguments for the script, as `nmc.args`.
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
    /// Print the TypeScript declarations of the `nmc` API (for editors)
    /// and exit.
    #[arg(long)]
    pub types: bool,
    /// Print the guide for writing and testing scripts and plugins (for
    /// people and AI assistants, e.g. saved as AGENTS.md) and exit.
    #[arg(long, conflicts_with = "types")]
    pub guide: bool,
    /// Start the script as if from the panels: the active panel's folder,
    /// for `nmc.context` (the desktop window runs panel scripts this way).
    #[arg(long, value_name = "LOCATION")]
    pub panel: Option<String>,
    /// With --panel: the other panel's folder.
    #[arg(long, value_name = "LOCATION", requires = "panel")]
    pub other: Option<String>,
    /// With --panel: a selected entry (repeatable).
    #[arg(long, value_name = "LOCATION", requires = "panel")]
    pub selected: Vec<String>,
}

#[derive(Debug, Args)]
#[command(after_help = "The script or plugin runs in a new temporary folder holding sample \
    files (or a copy of --fixture): `panel/`, the active panel, and `other/`, the other \
    panel. It may change only those. The report lists the checks (manifest, syntax, \
    types with `tsc` if installed), what each run printed, and the files it created, \
    removed and changed. Exit status 0 means every check and run passed.\n\nExamples:\n  \
    nmc test tidy.ts\n  nmc test tidy.ts --fixture ./samples --json -- $PANEL --dry-run\n  \
    nmc test ./my-plugin --command sort --select photo.JPG")]
pub struct TestArgs {
    /// A script (.ts or .js) or a plugin folder (with plugin.json).
    pub target: String,
    /// Arguments for a script, as `nmc.args`; `$PANEL` and `$OTHER` stand
    /// for the test folders. Default: `$PANEL`.
    #[arg(last = true)]
    pub args: Vec<String>,
    /// Copy this folder into the test panel instead of the sample files.
    #[arg(long, value_name = "DIR")]
    pub fixture: Option<String>,
    /// For a plugin: test only this command (repeatable). Default: all.
    #[arg(long, value_name = "ID")]
    pub command: Vec<String>,
    /// Select only this entry of the test panel (repeatable). Default:
    /// every entry.
    #[arg(long, value_name = "NAME")]
    pub select: Vec<String>,
    /// Stop a script after this many seconds (plugins use their manifest).
    #[arg(long, value_name = "SECONDS", default_value_t = 30.0)]
    pub timeout: f64,
    /// Print the report as JSON.
    #[arg(long)]
    pub json: bool,
    /// Keep the test folder and say where it is.
    #[arg(long)]
    pub keep: bool,
}

#[derive(Debug, Args)]
pub struct AiArgs {
    #[command(subcommand)]
    pub action: AiAction,
}

#[derive(Debug, Subcommand)]
pub enum AiAction {
    /// Write a script (or a plugin) from a description: Claude writes it,
    /// `nmc test` tries it on sample files, and Claude fixes what failed.
    /// A script that passes is saved among your scripts (F9, Scripts).
    #[command(after_help = "Needs an Anthropic API key: ANTHROPIC_API_KEY, or one saved \
        with `nmc ai key`. Each try is one request to the Claude API, billed to that key.\n\n\
        Examples:\n  nmc ai new \"move files older than 30 days into a folder named old\"\n  \
        nmc ai new --plugin \"count the lines of the selected text files\"")]
    New {
        /// What the script should do, in any language.
        #[arg(required = true, num_args = 1..)]
        description: Vec<String>,
        /// Write a plugin (installed when it passes) instead of a script.
        #[arg(long)]
        plugin: bool,
        /// Save here instead: a file for a script, a folder for a plugin.
        #[arg(long, value_name = "PATH")]
        out: Option<String>,
        /// Test on a copy of this folder instead of the sample files.
        #[arg(long, value_name = "DIR")]
        fixture: Option<String>,
        /// How many times Claude may try.
        #[arg(long, value_name = "N", default_value_t = 3)]
        tries: u32,
        /// The Claude model.
        #[arg(long, default_value = "claude-opus-5-5")]
        model: String,
        /// Replace a script or plugin with the same name.
        #[arg(long)]
        force: bool,
    },
    /// Have Claude write the multi-rename rule (template, find/replace or
    /// regex, case, counter) for a description, and preview the new names
    /// (marking those that already exist). Renames nothing: run the
    /// printed `nmc rename` to apply it.
    Rename {
        /// How to rename, in any language, e.g. "date first, as YYYY-MM-DD".
        description: String,
        /// The files (only their names are sent).
        #[arg(required = true)]
        paths: Vec<String>,
        /// The Claude model.
        #[arg(long, default_value = "claude-opus-5-5")]
        model: String,
        /// Print the rule and the new names as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Save an Anthropic API key (read from standard input) for `nmc ai`.
    Key,
}

#[derive(Debug, Args)]
pub struct DesktopArgs {
    #[command(subcommand)]
    pub action: DesktopAction,
}

#[derive(Debug, Subcommand)]
pub enum DesktopAction {
    /// Write the launcher (applications menu, dock) with the current icon.
    /// The icon is named by its content, so the desktop never shows an old
    /// one. Run it again after an update; the window also does it by itself
    /// when its launcher exists.
    Install,
    /// Remove the launcher and the icons written by `install`.
    Remove,
}

#[derive(Debug, Args)]
pub struct PluginArgs {
    #[command(subcommand)]
    pub action: PluginAction,
}

#[derive(Debug, Subcommand)]
pub enum RepoAction {
    /// The repositories plugins can be installed from.
    List,
    /// Add a repository: a URL, git@host:owner/repo or github:owner/repo, with
    /// #branch-or-tag to pick a version.
    Add {
        /// The repository.
        repository: String,
        /// Do not ask for confirmation.
        #[arg(short, long)]
        yes: bool,
    },
    /// Remove a repository (plugins installed from it stay installed).
    Remove {
        /// The repository, as `nmc plugin repo list` shows it.
        repository: String,
    },
    /// Show or hide the official repository.
    Official {
        /// `on` or `off`.
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum PluginAction {
    /// The installed plugins, their commands and permissions.
    List,
    /// The plugins of the configured repositories (the official one and yours).
    Available {
        /// Only plugins whose name or description contains this text.
        #[arg(long, value_name = "TEXT")]
        search: Option<String>,
        /// Fetch the repositories now (otherwise a list fetched today is used).
        #[arg(long)]
        refresh: bool,
    },
    /// Add, list or remove plugin repositories.
    Repo {
        #[command(subcommand)]
        action: RepoAction,
    },
    /// Install (or replace) a plugin by name, or from a folder or a Git repository.
    #[command(after_help = "The source is a plugin's name from `nmc plugin available` (NAME, or \
NAME@REPOSITORY when several repositories have it), a folder with a plugin.json, or a Git \
repository (cloned with the system's git): a URL, git@host:owner/repo or github:owner/repo. Add \
#branch-or-tag to pick a version. Repository installs are remembered in plugin-lock.json \
(see `nmc plugin update`).")]
    Install {
        /// A plugin's name, a folder with a plugin.json, or a Git repository.
        source: String,
        /// The folder of the repository that holds the plugin (default: its top).
        #[arg(long, value_name = "FOLDER")]
        subdir: Option<String>,
    },
    /// Fetch newer versions of plugins installed from Git.
    Update {
        /// The plugins to update (default: all installed from Git).
        names: Vec<String>,
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

/// Options of `nmc pick`.
#[derive(Debug, Args)]
pub struct PickArgs {
    /// Folder or URI to start in (the current folder when omitted).
    pub location: Option<String>,
    /// Choose a folder (the one shown, with Ctrl+F10) instead of files.
    #[arg(short, long)]
    pub dir: bool,
    /// Allow several files: mark them with Space, then press Enter.
    #[arg(short, long)]
    pub multiple: bool,
    /// End each path with a NUL character instead of a new line.
    #[arg(short = '0', long)]
    pub null: bool,
}

/// Arguments of `nmc mcp`.
#[derive(Debug, Args)]
#[command(
    after_help = "Nothing is allowed by default: grant folders (or sftp://, s3:// locations). \
Set it up in Claude Code with: claude mcp add nmc -- nmc mcp --read ~/Documents --write ~/Documents/out"
)]
pub struct McpArgs {
    /// A location the assistant may read (repeatable).
    #[arg(long, value_name = "LOCATION")]
    pub read: Vec<String>,
    /// A location the assistant may read and change (repeatable).
    #[arg(long, value_name = "LOCATION")]
    pub write: Vec<String>,
    /// A JSON file with {"read": [...], "write": [...]}, added to the above.
    #[arg(long, value_name = "FILE")]
    pub grants: Option<std::path::PathBuf>,
    /// Append one line per tool call (time, tool, arguments) to this file.
    #[arg(long, value_name = "FILE")]
    pub log: Option<std::path::PathBuf>,
}
