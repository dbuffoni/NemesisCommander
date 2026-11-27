//! Where an S3 connection gets its keys, region and endpoint: the same
//! environment variables and `~/.aws` files as the AWS command-line tools,
//! so a working `aws s3 ls` setup works here too.
//!
//! `s3://bucket/...` uses the environment, then the profile named by
//! `AWS_PROFILE` (or `default`). `s3://name@bucket/...` uses profile
//! `name` only, which is how one machine reaches several services (AWS,
//! MinIO, Cloudflare R2, ...): each profile can set its own
//! `endpoint_url` and `region`.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::sigv4::Credentials;

/// Everything needed to talk to one S3 service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// `None` sends unsigned requests (public buckets).
    pub credentials: Option<Credentials>,
    /// Signing region (`us-east-1` when nothing says otherwise).
    pub region: String,
    /// A custom endpoint such as `http://localhost:9000`; `None` is AWS.
    pub endpoint: Option<String>,
}

/// Keys typed for this session (`profile` → keys), used before the
/// environment and the files. They live in memory only.
fn session() -> std::sync::MutexGuard<'static, HashMap<String, Credentials>> {
    static SESSION: std::sync::OnceLock<std::sync::Mutex<HashMap<String, Credentials>>> =
        std::sync::OnceLock::new();
    SESSION.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The profile name keys are kept under: the one in the address, or `default`.
fn profile_name(profile: Option<&str>) -> String {
    profile.filter(|p| !p.is_empty()).unwrap_or("default").to_owned()
}

/// Uses these keys for `profile` until the program ends.
pub fn set_session(profile: Option<&str>, access_key: &str, secret_key: &str) {
    let keys = Credentials {
        access_key: access_key.to_owned(),
        secret_key: secret_key.to_owned(),
        session_token: None,
    };
    session().insert(profile_name(profile), keys);
}

/// Whether keys were given for `profile` in this session.
pub fn has_session(profile: Option<&str>) -> bool {
    session().contains_key(&profile_name(profile))
}

/// Uses `path` as the credentials file when saving keys (`None`: the usual
/// one). For tests and embedding.
pub fn set_credentials_file(path: Option<PathBuf>) {
    *file_override() = path;
}

fn file_override() -> std::sync::MutexGuard<'static, Option<PathBuf>> {
    static FILE: std::sync::OnceLock<std::sync::Mutex<Option<PathBuf>>> =
        std::sync::OnceLock::new();
    FILE.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner())
}

/// The credentials file: `AWS_SHARED_CREDENTIALS_FILE` or `~/.aws/credentials`.
fn credentials_path() -> Option<PathBuf> {
    if let Some(path) = file_override().clone() {
        return Some(path);
    }
    std::env::var_os("AWS_SHARED_CREDENTIALS_FILE")
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join(".aws/credentials")))
}

/// Writes the keys of `profile` in the credentials file (readable only by the
/// user), keeping its other profiles and settings. Returns the file.
pub fn save(profile: Option<&str>, access_key: &str, secret_key: &str) -> std::io::Result<PathBuf> {
    let bad = |text: &str| text.is_empty() || text.contains(char::is_control);
    if bad(access_key) || bad(secret_key) {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty or odd keys"));
    }
    let path = credentials_path().ok_or_else(|| std::io::Error::other("no home folder"))?;
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let text = with_keys(&old, &profile_name(profile), access_key, secret_key);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(text.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, text)?;
    Ok(path)
}

/// `text` with the two keys of `[name]` set, other lines kept.
fn with_keys(text: &str, name: &str, access_key: &str, secret_key: &str) -> String {
    let set = |key: &str, value: &str| format!("{key} = {value}");
    let is_key = |line: &str, key: &str| {
        line.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))
    };
    let mut out: Vec<String> = Vec::new();
    let (mut inside, mut found) = (false, false);
    let flush = |out: &mut Vec<String>| {
        out.push(set("aws_access_key_id", access_key));
        out.push(set("aws_secret_access_key", secret_key));
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(header) = trimmed.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if inside {
                flush(&mut out);
                inside = false;
            }
            if header.trim() == name {
                inside = true;
                found = true;
            }
            out.push(line.to_owned());
            continue;
        }
        if inside
            && (is_key(trimmed, "aws_access_key_id")
                || is_key(trimmed, "aws_secret_access_key")
                || is_key(trimmed, "aws_session_token"))
        {
            continue;
        }
        out.push(line.to_owned());
    }
    if inside {
        flush(&mut out);
    }
    if !found {
        if out.last().is_some_and(|l| !l.trim().is_empty()) {
            out.push(String::new());
        }
        out.push(format!("[{name}]"));
        flush(&mut out);
    }
    out.join("\n") + "\n"
}

/// The process environment, or a fixed set of variables in tests.
pub type Environment = HashMap<String, String>;

/// Reads the settings for `profile` (from the location) using the real
/// environment and files.
pub fn resolve(profile: Option<&str>) -> Settings {
    let env: Environment = std::env::vars().collect();
    let home = directories::BaseDirs::new().map(|d| d.home_dir().join(".aws"));
    let file = |var: &str, name: &str| {
        env.get(var).map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(name)))
    };
    let read = |path: Option<PathBuf>| {
        path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default()
    };
    let credentials = read(file("AWS_SHARED_CREDENTIALS_FILE", "credentials"));
    let config = read(file("AWS_CONFIG_FILE", "config"));
    let mut settings = resolve_from(profile, &env, &credentials, &config);
    if let Some(keys) = session().get(&profile_name(profile)) {
        settings.credentials = Some(keys.clone());
    }
    settings
}

/// [`resolve`] with explicit inputs.
pub fn resolve_from(
    profile: Option<&str>,
    env: &Environment,
    credentials_file: &str,
    config_file: &str,
) -> Settings {
    let var = |name: &str| env.get(name).filter(|v| !v.is_empty()).cloned();
    let name = profile
        .map(str::to_owned)
        .or_else(|| var("AWS_PROFILE"))
        .unwrap_or_else(|| "default".to_owned());
    let keys = section(credentials_file, &name);
    // In the config file, profiles other than default are `[profile name]`.
    let config_name = if name == "default" { name.clone() } else { format!("profile {name}") };
    let config = section(config_file, &config_name);
    let setting = |key: &str| keys.get(key).or_else(|| config.get(key)).cloned();

    // An explicit profile ignores keys in the environment.
    let from_env = profile
        .is_none()
        .then(|| var("AWS_ACCESS_KEY_ID"))
        .flatten()
        .zip(var("AWS_SECRET_ACCESS_KEY"));
    let credentials = match from_env {
        Some((access_key, secret_key)) => {
            Some(Credentials { access_key, secret_key, session_token: var("AWS_SESSION_TOKEN") })
        }
        None => setting("aws_access_key_id").zip(setting("aws_secret_access_key")).map(
            |(access_key, secret_key)| Credentials {
                access_key,
                secret_key,
                session_token: setting("aws_session_token"),
            },
        ),
    };
    let env_first = |names: &[&str], key: &str| {
        let from_env = names.iter().find_map(|n| var(n));
        if profile.is_some() {
            setting(key).or(from_env)
        } else {
            from_env.or_else(|| setting(key))
        }
    };
    Settings {
        credentials,
        region: env_first(&["AWS_REGION", "AWS_DEFAULT_REGION"], "region")
            .unwrap_or_else(|| "us-east-1".to_owned()),
        endpoint: env_first(&["AWS_ENDPOINT_URL_S3", "AWS_ENDPOINT_URL"], "endpoint_url")
            .map(|e| e.trim_end_matches('/').to_owned()),
    }
}

/// The `key = value` pairs of `[name]` in an INI file.
fn section(text: &str, name: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.is_empty() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            inside = header.trim() == name;
            continue;
        }
        if inside && let Some((key, value)) = line.split_once('=') {
            values.insert(key.trim().to_lowercase(), value.trim().to_owned());
        }
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREDENTIALS: &str = "
        [default]
        aws_access_key_id = AKDEFAULT
        aws_secret_access_key = secret-default

        [minio]
        aws_access_key_id=AKMINIO
        aws_secret_access_key=secret-minio
    ";
    const CONFIG: &str = "
        [default]
        region = eu-south-1
        # a comment
        [profile minio]
        region = us-east-1
        endpoint_url = http://nas.local:9000/
    ";

    fn env(pairs: &[(&str, &str)]) -> Environment {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn keys_are_set_in_their_profile_and_the_rest_is_kept() {
        let text = "[default]\naws_access_key_id = A\nregion = x\n\n[minio]\naws_access_key_id=B\naws_secret_access_key=C\n";
        let new = with_keys(text, "minio", "NEW", "SECRET");
        assert_eq!(
            new,
            "[default]\naws_access_key_id = A\nregion = x\n\n[minio]\naws_access_key_id = NEW\naws_secret_access_key = SECRET\n"
        );
        let added = with_keys(text, "r2", "K", "S");
        assert!(added.ends_with("\n\n[r2]\naws_access_key_id = K\naws_secret_access_key = S\n"));
        assert!(added.starts_with(text.trim_end()));
        let again = with_keys(&with_keys("", "d", "1", "2"), "d", "3", "4");
        assert_eq!(again, "[d]\naws_access_key_id = 3\naws_secret_access_key = 4\n");
    }

    #[test]
    fn session_keys_come_first() {
        set_session(Some("sessionprofile"), "AKSESSION", "sk");
        assert!(has_session(Some("sessionprofile")));
        assert!(!has_session(Some("other")));
        let s = resolve(Some("sessionprofile"));
        assert_eq!(s.credentials.unwrap().access_key, "AKSESSION");
    }

    #[test]
    fn default_profile_from_files() {
        let s = resolve_from(None, &env(&[]), CREDENTIALS, CONFIG);
        assert_eq!(s.credentials.unwrap().access_key, "AKDEFAULT");
        assert_eq!((s.region.as_str(), s.endpoint), ("eu-south-1", None));
    }

    #[test]
    fn a_named_profile_brings_its_endpoint() {
        let e = env(&[("AWS_ACCESS_KEY_ID", "AKENV"), ("AWS_SECRET_ACCESS_KEY", "x")]);
        let s = resolve_from(Some("minio"), &e, CREDENTIALS, CONFIG);
        assert_eq!(s.credentials.unwrap().access_key, "AKMINIO", "env keys are ignored");
        assert_eq!(s.endpoint.as_deref(), Some("http://nas.local:9000"));
    }

    #[test]
    fn the_environment_wins_without_a_profile() {
        let e = env(&[
            ("AWS_ACCESS_KEY_ID", "AKENV"),
            ("AWS_SECRET_ACCESS_KEY", "x"),
            ("AWS_SESSION_TOKEN", "token"),
            ("AWS_REGION", "ap-south-2"),
            ("AWS_ENDPOINT_URL", "http://127.0.0.1:9000"),
        ]);
        let s = resolve_from(None, &e, CREDENTIALS, CONFIG);
        let credentials = s.credentials.unwrap();
        assert_eq!(credentials.access_key, "AKENV");
        assert_eq!(credentials.session_token.as_deref(), Some("token"));
        assert_eq!(s.region, "ap-south-2");
        assert_eq!(s.endpoint.as_deref(), Some("http://127.0.0.1:9000"));

        let picked = resolve_from(None, &env(&[("AWS_PROFILE", "minio")]), CREDENTIALS, CONFIG);
        assert_eq!(picked.credentials.unwrap().access_key, "AKMINIO");
    }

    #[test]
    fn nothing_configured_means_anonymous_us_east_1() {
        let s = resolve_from(None, &env(&[]), "", "");
        assert_eq!(s, Settings { credentials: None, region: "us-east-1".into(), endpoint: None });
    }
}
