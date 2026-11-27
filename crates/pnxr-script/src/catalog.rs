//! Plugin repositories (ADR 0054): a Git repository with a `plugins/`
//! folder holding one folder per plugin, each with its `plugin.json`. The
//! catalogue is what those manifests say; nothing else has to be kept up to
//! date. A repository can also list plugins that live in other repositories
//! in `plugins/index.json`.
//!
//! The official repository is built in. Catalogues are cached on disk, so
//! browsing works offline and the network is used at most once a day (or
//! when asked).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::plugin::{self, Plugin, PluginError};
use crate::source::{self, Lock, Repository};

/// The official repository: this project's own `plugins/` folder.
pub const OFFICIAL_URL: &str = "https://github.com/dbuffoni/Pairnox.git";

/// The folder of a repository that holds the plugins.
pub const PLUGINS_FOLDER: &str = "plugins";

/// The optional list of plugins that live elsewhere.
pub const INDEX: &str = "index.json";

/// A cached catalogue older than this is fetched again when it is used.
pub const MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// A repository, as the settings keep it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositorySpec {
    /// What `git clone` accepts.
    pub url: String,
    /// A branch or tag (the default branch when absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

impl RepositorySpec {
    /// The official repository.
    pub fn official() -> Self {
        RepositorySpec { url: OFFICIAL_URL.into(), reference: None }
    }

    /// The repository `text` names (`github:owner/repo#ref`, a URL...).
    pub fn parse(text: &str) -> Option<Self> {
        source::parse_repository(text)
            .map(|Repository { url, reference }| RepositorySpec { url, reference })
    }

    /// Whether this is the official repository.
    pub fn is_official(&self) -> bool {
        same_url(&self.url, OFFICIAL_URL)
    }

    /// How it is written back: `url` or `url#ref`.
    pub fn label(&self) -> String {
        match &self.reference {
            Some(reference) => format!("{}#{reference}", self.url),
            None => self.url.clone(),
        }
    }

    fn as_source(&self) -> Repository {
        Repository { url: self.url.clone(), reference: self.reference.clone() }
    }
}

fn same_url(a: &str, b: &str) -> bool {
    let normal = |url: &str| url.trim_end_matches('/').trim_end_matches(".git").to_lowercase();
    normal(a) == normal(b)
}

/// The repositories to use: the official one (unless switched off), then the
/// user's, without repeats.
pub fn repositories(official: bool, extra: &[RepositorySpec]) -> Vec<RepositorySpec> {
    let mut list = Vec::new();
    if official {
        list.push(RepositorySpec::official());
    }
    for repository in extra {
        if !list.iter().any(|r: &RepositorySpec| {
            same_url(&r.url, &repository.url) && r.reference == repository.reference
        }) {
            list.push(repository.clone());
        }
    }
    list
}

/// One plugin of a catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The plugin's name.
    pub name: String,
    /// Its latest version.
    pub version: String,
    /// One line about it.
    pub description: String,
    /// Who wrote it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Its licence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// The repository to install it from.
    pub repository: String,
    /// Branch or tag of that repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    /// The folder of the repository holding it.
    pub subdir: String,
    /// The repository whose catalogue lists it (differs from `repository`
    /// for plugins listed in `plugins/index.json`).
    pub listed_in: String,
    /// Whether the official repository lists it.
    pub official: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    fetched_at: u64,
    commit: String,
    entries: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct IndexFile {
    plugins: Vec<IndexItem>,
}

#[derive(Debug, Deserialize)]
struct IndexItem {
    name: String,
    #[serde(default)]
    version: String,
    #[serde(default)]
    description: String,
    repository: String,
    #[serde(default, rename = "ref")]
    reference: Option<String>,
    #[serde(default)]
    subdir: Option<String>,
}

/// The plugins of a checked-out repository (its `plugins/` folder), and
/// what was wrong with the ones that cannot be listed.
pub fn scan(root: &Path, spec: &RepositorySpec) -> (Vec<Entry>, Vec<String>) {
    let folder = root.join(PLUGINS_FOLDER);
    let official = spec.is_official();
    let (mut entries, mut problems) = (Vec::new(), Vec::new());
    let mut names: Vec<_> = match fs::read_dir(&folder) {
        Ok(read) => read.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name()).collect(),
        Err(_) => {
            problems.push(format!("{}: no {PLUGINS_FOLDER}/ folder", spec.url));
            return (entries, problems);
        }
    };
    names.sort();
    for name in names {
        let dir = folder.join(&name);
        match Plugin::load(&dir) {
            Ok(plugin) => {
                let m = plugin.manifest;
                entries.push(Entry {
                    name: m.name,
                    version: m.version,
                    description: m.description,
                    author: m.author,
                    license: m.license,
                    repository: spec.url.clone(),
                    reference: spec.reference.clone(),
                    subdir: format!("{PLUGINS_FOLDER}/{}", name.to_string_lossy()),
                    listed_in: spec.url.clone(),
                    official,
                });
            }
            Err(PluginError::Manifest { message, .. }) if !dir.join(plugin::MANIFEST).exists() => {
                problems.push(format!("{}: {message}", dir.display()));
            }
            Err(e) => problems.push(format!("{}: {e}", spec.url)),
        }
    }
    if let Ok(text) = fs::read_to_string(folder.join(INDEX)) {
        match serde_json::from_str::<IndexFile>(&text) {
            Ok(index) => {
                for item in index.plugins {
                    let Some(target) = RepositorySpec::parse(&item.repository) else {
                        problems
                            .push(format!("{}: {:?} is not a repository", INDEX, item.repository));
                        continue;
                    };
                    entries.push(Entry {
                        name: item.name,
                        version: item.version,
                        description: item.description,
                        author: None,
                        license: None,
                        repository: target.url,
                        reference: item.reference.or(target.reference),
                        subdir: item.subdir.unwrap_or_default(),
                        listed_in: spec.url.clone(),
                        official,
                    });
                }
            }
            Err(e) => problems.push(format!("{}/{INDEX}: {e}", PLUGINS_FOLDER)),
        }
    }
    (entries, problems)
}

fn cache_file(cache_dir: &Path, spec: &RepositorySpec) -> PathBuf {
    // FNV-1a: stable between versions, unlike `DefaultHasher`.
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in spec.label().bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
    }
    cache_dir.join(format!("{hash:016x}.json"))
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn read_cache(cache_dir: &Path, spec: &RepositorySpec) -> Option<Cache> {
    serde_json::from_str(&fs::read_to_string(cache_file(cache_dir, spec)).ok()?).ok()
}

/// What the cache holds for `spec`, however old.
pub fn cached(cache_dir: &Path, spec: &RepositorySpec) -> Option<Vec<Entry>> {
    read_cache(cache_dir, spec).map(|c| c.entries)
}

/// Fetches the repository and rebuilds its cached catalogue.
pub fn refresh(
    cache_dir: &Path,
    spec: &RepositorySpec,
) -> Result<(Vec<Entry>, Vec<String>), PluginError> {
    let checkout = source::fetch(&spec.url, spec.reference.as_deref())?;
    let (entries, problems) = scan(&checkout.plugin_dir(None)?, spec);
    let failed = |e: String| PluginError::Source(format!("{}: {e}", cache_dir.display()));
    fs::create_dir_all(cache_dir).map_err(|e| failed(e.to_string()))?;
    let cache = Cache { fetched_at: now(), commit: checkout.commit.clone(), entries };
    let text = serde_json::to_string_pretty(&cache).map_err(|e| failed(e.to_string()))?;
    fs::write(cache_file(cache_dir, spec), text).map_err(|e| failed(e.to_string()))?;
    Ok((cache.entries, problems))
}

/// The catalogue of every repository in `specs`. A catalogue younger than
/// [`MAX_AGE`] comes from the cache unless `force`; when fetching fails the
/// cache is used however old and the problem is reported. A repository
/// that was never fetched and cannot be now only adds a problem.
pub fn available(
    cache_dir: &Path,
    specs: &[RepositorySpec],
    force: bool,
) -> (Vec<Entry>, Vec<String>) {
    let (mut entries, mut problems) = (Vec::new(), Vec::new());
    for spec in specs {
        let cache = read_cache(cache_dir, spec);
        let fresh =
            cache.as_ref().is_some_and(|c| now().saturating_sub(c.fetched_at) < MAX_AGE.as_secs());
        if fresh && !force {
            entries.extend(cache.map(|c| c.entries).unwrap_or_default());
            continue;
        }
        match refresh(cache_dir, spec) {
            Ok((list, found)) => {
                entries.extend(list);
                problems.extend(found);
            }
            Err(e) => match cache {
                Some(old) => {
                    problems.push(format!("{}: {e} (showing the saved list)", spec.url));
                    entries.extend(old.entries);
                }
                None => problems.push(format!("{}: {e}", spec.url)),
            },
        }
    }
    (entries, problems)
}

/// Picks the entry named `wanted`: `name` or `name@repository` (a part of
/// the repository's URL, or `official`). A name in several repositories
/// goes to the official one, and is otherwise an error naming the choices.
pub fn find<'a>(entries: &'a [Entry], wanted: &str) -> Result<&'a Entry, PluginError> {
    let (name, from) = match wanted.split_once('@') {
        Some((name, from)) => (name, Some(from)),
        None => (wanted, None),
    };
    let mut found: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.name == name)
        .filter(|e| match from {
            None => true,
            Some("official") => e.official,
            Some(part) => e.listed_in.to_lowercase().contains(&part.to_lowercase()),
        })
        .collect();
    found.dedup_by(|a, b| a.repository == b.repository && a.subdir == b.subdir);
    match found.as_slice() {
        [] => Err(PluginError::NotFound(format!(
            "no plugin {name:?} in the repositories (see `pnxr plugin available`)"
        ))),
        [one] => Ok(one),
        many => {
            if let Some(official) = many.iter().find(|e| e.official) {
                return Ok(official);
            }
            let places: Vec<_> = many.iter().map(|e| e.listed_in.as_str()).collect();
            Err(PluginError::NotFound(format!(
                "{name} is in several repositories ({}): say which, as {name}@REPOSITORY",
                places.join(", ")
            )))
        }
    }
}

/// Installs the catalogue entry and records its source.
pub fn install(entry: &Entry, plugins_dir: &Path, lock: &mut Lock) -> Result<Plugin, PluginError> {
    let spec = RepositorySpec { url: entry.repository.clone(), reference: entry.reference.clone() };
    let subdir = (!entry.subdir.is_empty()).then_some(entry.subdir.as_str());
    source::install(&spec.as_source(), subdir, plugins_dir, lock)
}

/// A catalogue line with what is known locally about the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// The catalogue entry.
    pub entry: Entry,
    /// The installed version, when it is installed.
    pub installed: Option<String>,
    /// The installed copy is older than the catalogue's.
    pub update: bool,
}

impl Row {
    /// `Official`, or `Unofficial` followed by where it comes from.
    pub fn origin(&self) -> String {
        if self.entry.official {
            "Official".into()
        } else {
            format!("Unofficial ({})", self.entry.listed_in)
        }
    }

    /// `installed 1.0`, `installed 1.0, update 1.1` or empty.
    pub fn state(&self) -> String {
        match (&self.installed, self.update) {
            (Some(v), true) => format!("installed {v}, update {}", self.entry.version),
            (Some(v), false) => format!("installed {v}"),
            (None, _) => String::new(),
        }
    }
}

/// The catalogue lines for `entries` (filtered by `search`, matched on the
/// name and description), with what is installed in `plugins_dir`.
pub fn rows(entries: &[Entry], search: &str, plugins_dir: &Path) -> Vec<Row> {
    let (installed, _) = plugin::discover(plugins_dir);
    let needle = search.to_lowercase();
    entries
        .iter()
        .filter(|e| {
            needle.is_empty()
                || e.name.to_lowercase().contains(&needle)
                || e.description.to_lowercase().contains(&needle)
        })
        .map(|entry| {
            let have = installed.iter().find(|p| p.name() == entry.name);
            Row {
                installed: have.map(|p| p.manifest.version.clone()),
                update: have.is_some_and(|p| p.manifest.version != entry.version),
                entry: entry.clone(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(root: &Path, folder: &str, name: &str, version: &str) {
        let dir = root.join("plugins").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("plugin.json"),
            format!(
                r#"{{"name":"{name}","version":"{version}","description":"{name} plugin",
                    "commands":[{{"id":"go","title":"Go"}}]}}"#
            ),
        )
        .unwrap();
        fs::write(dir.join("main.ts"), "pnxr.print('hi');").unwrap();
    }

    fn entry(name: &str, repo: &str, official: bool) -> Entry {
        Entry {
            name: name.into(),
            version: "1".into(),
            description: String::new(),
            author: None,
            license: None,
            repository: repo.into(),
            reference: None,
            subdir: format!("plugins/{name}"),
            listed_in: repo.into(),
            official,
        }
    }

    #[test]
    fn the_catalogue_is_the_manifests_under_plugins() {
        let dir = tempfile::tempdir().unwrap();
        plugin(dir.path(), "a", "a", "1.0.0");
        plugin(dir.path(), "b", "b", "2.1.0");
        fs::create_dir_all(dir.path().join("plugins/broken")).unwrap();
        fs::write(
            dir.path().join("plugins/index.json"),
            r#"{"plugins":[{"name":"far","version":"3","description":"elsewhere",
                "repository":"github:me/far","ref":"v3","subdir":"x"}]}"#,
        )
        .unwrap();
        let spec = RepositorySpec::parse("https://example.com/me/repo.git").unwrap();
        let (entries, problems) = scan(dir.path(), &spec);
        let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.subdir.as_str())).collect();
        assert_eq!(names, [("a", "plugins/a"), ("b", "plugins/b"), ("far", "x")]);
        assert_eq!(entries[2].repository, "https://github.com/me/far.git");
        assert_eq!(entries[2].reference.as_deref(), Some("v3"));
        assert_eq!(entries[2].listed_in, spec.url);
        assert!(entries.iter().all(|e| !e.official));
        assert_eq!(problems.len(), 1, "{problems:?}");
    }

    #[test]
    fn rows_say_where_a_plugin_comes_from_and_what_is_installed() {
        let root = tempfile::tempdir().unwrap();
        plugin(root.path(), "x", "x", "1");
        let dir = root.path().join("plugins");
        let mut newer = entry("x", OFFICIAL_URL, true);
        newer.version = "2".into();
        let list = [entry("x", OFFICIAL_URL, true), entry("y", "https://other/y.git", false)];
        let found = rows(&list, "", &dir);
        assert_eq!(found[0].origin(), "Official");
        assert_eq!(found[1].origin(), "Unofficial (https://other/y.git)");
        assert_eq!(found[0].state(), "installed 1");
        assert!(found[1].installed.is_none() && found[1].state().is_empty());
        let found = rows(&[newer], "", &dir);
        assert_eq!(found[0].state(), "installed 1, update 2");
        assert_eq!(rows(&list, "Y", &dir).len(), 1);
    }

    #[test]
    fn a_repository_without_plugins_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let spec = RepositorySpec::parse("github:me/none").unwrap();
        let (entries, problems) = scan(dir.path(), &spec);
        assert!(entries.is_empty() && problems[0].contains("no plugins/ folder"));
    }

    #[test]
    fn official_comes_first_and_repeats_are_dropped() {
        let extra = [
            RepositorySpec::parse("github:me/more").unwrap(),
            RepositorySpec::parse("github:me/more").unwrap(),
            RepositorySpec::parse("https://github.com/dbuffoni/Pairnox").unwrap(),
        ];
        let list = repositories(true, &extra);
        assert!(list[0].is_official());
        assert_eq!(list.len(), 2, "the official one listed again is the same");
        assert_eq!(repositories(false, &extra).len(), 2);
        assert!(!repositories(false, &extra[..1])[0].is_official());
    }

    #[test]
    fn a_name_in_two_repositories_needs_a_choice_unless_official() {
        let list = [entry("x", "https://a/x.git", false), entry("x", "https://b/x.git", false)];
        let error = find(&list, "x").unwrap_err().to_string();
        assert!(error.contains("x@REPOSITORY"), "{error}");
        assert_eq!(find(&list, "x@b/").unwrap().repository, "https://b/x.git");
        let with_official = [list[0].clone(), entry("x", OFFICIAL_URL, true)];
        assert!(find(&with_official, "x").unwrap().official);
        assert!(find(&with_official, "x@https://a").is_ok());
        assert!(find(&list, "nope").is_err());
    }

    #[test]
    fn the_cache_serves_a_catalogue_when_the_network_does_not() {
        let cache = tempfile::tempdir().unwrap();
        let spec = RepositorySpec::parse("file:///nonexistent/repo.git").unwrap();
        // Nothing cached and nothing to fetch: a problem, no entries.
        let (entries, problems) = available(cache.path(), std::slice::from_ref(&spec), false);
        assert!(entries.is_empty() && problems.len() == 1);
        // An old cache is shown with a note.
        let old = Cache {
            fetched_at: 1,
            commit: "c".into(),
            entries: vec![entry("x", &spec.url, false)],
        };
        fs::write(cache_file(cache.path(), &spec), serde_json::to_string(&old).unwrap()).unwrap();
        let (entries, problems) = available(cache.path(), std::slice::from_ref(&spec), false);
        assert_eq!(entries.len(), 1);
        assert!(problems[0].contains("saved list"), "{problems:?}");
    }
}
