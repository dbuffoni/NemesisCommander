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
    resolve_from(profile, &env, &credentials, &config)
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
