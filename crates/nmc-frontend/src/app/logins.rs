//! The login of a connection that needs a password: where it is kept
//! (asked every time, or saved in the system keyring), and testing a connection
//! while it is being created.

use nmc_vfs::{VfsPath, keyring, netrc};

use super::App;
use crate::connections::{
    self, check, host_of, profile_of, user_of, uses_keys, uses_password, with_user,
};
use crate::dialogs::{
    ChooseDialog, ChoosePurpose, InputDialog, InputPurpose, Overlay, S3Next, Secret,
};

/// The connection being edited: its name and location now.
type Replace = Option<(String, String)>;

/// Whether `name` is acceptable as a user name.
fn valid_user(user: &str) -> bool {
    !user.is_empty() && !user.contains(|c: char| c.is_whitespace() || c == '@' || c == ':')
}

impl App {
    /// The questions for a new name field: the test hint under it.
    pub(super) fn ask_name(
        &mut self,
        title: &str,
        prompt: &str,
        initial: &str,
        purpose: InputPurpose,
    ) {
        self.overlay = Some(Overlay::Input(InputDialog::new(title, prompt, initial, purpose)));
    }

    /// The question for the address, with the examples showing at once.
    pub(super) fn ask_address(&mut self, title: &str, initial: &str, purpose: InputPurpose) {
        let mut dialog = InputDialog::new(title, connections::prompt(), initial, purpose);
        dialog.preview = connections::hints(initial);
        self.overlay = Some(Overlay::Input(dialog));
    }

    /// The connection has its name and address: asks where the login is
    /// kept when it needs a password, then stores it.
    pub(super) fn finish_connection(
        &mut self,
        name: String,
        uri: String,
        replace: Replace,
        secret: Secret,
    ) {
        let needs_login = VfsPath::parse(&uri).is_ok_and(|p| uses_password(&p));
        if !needs_login {
            return self.store_connection(name, uri, replace, false);
        }
        let asks = replace.as_ref().is_some_and(|(_, old)| self.connection_asks(old));
        let parsed = VfsPath::parse(&uri).ok();
        let keys = parsed.as_ref().is_some_and(uses_keys);
        let ssh = parsed.as_ref().is_some_and(|p| matches!(p.scheme(), "sftp" | "agent"));
        let rows = if keys {
            vec![
                "Ask for the keys every time (kept in memory only)".to_owned(),
                "Save the keys in ~/.aws/credentials (a file only you can read)".to_owned(),
                "Keep it as it is (use the keys already set up: ~/.aws, environment)".to_owned(),
            ]
        } else {
            let keep = if ssh {
                "Keep it as it is (SSH keys, ssh-agent or ~/.ssh/config)"
            } else {
                "Keep it as it is (use the login ~/.netrc already has)"
            };
            let save = if keyring::available() {
                format!("Save the login in {}", keyring::store_name())
            } else {
                format!("Save the login in {} ({})", keyring::store_name(), keyring::missing())
            };
            vec![
                "Ask for the password every time (kept in memory only)".to_owned(),
                save,
                keep.to_owned(),
            ]
        };
        let title = if keys { "Keys for this connection" } else { "Password for this connection" };
        let mut dialog = ChooseDialog::new(
            title,
            rows,
            ChoosePurpose::ConnectionLogin { name, uri, replace, secret: secret.clone() },
        );
        dialog.cursor = if !secret.0.is_empty() {
            1
        } else if asks {
            0
        } else {
            2
        };
        dialog.footer = crate::hints::list("choose", &["Ctrl+T: test"], "cancel");
        self.overlay = Some(Overlay::Choose(dialog));
    }

    /// The answer of the login question.
    pub(super) fn chosen_login(&mut self, purpose: ChoosePurpose, cursor: usize) {
        let ChoosePurpose::ConnectionLogin { name, uri, replace, secret } = purpose else {
            return;
        };
        match cursor {
            0 => self.login_step(name, uri, replace, true, secret),
            1 => self.login_step(name, uri, replace, false, secret),
            _ => self.store_connection(name, uri, replace, false),
        }
    }

    /// Asks for the user name if the address has none, then goes on.
    fn login_step(
        &mut self,
        name: String,
        uri: String,
        replace: Replace,
        ask: bool,
        secret: Secret,
    ) {
        let parsed = VfsPath::parse(&uri);
        if parsed.as_ref().is_ok_and(uses_keys) {
            // S3 has keys, no user: the profile in the address is optional.
            if ask {
                return self.store_connection(name, uri, replace, true);
            }
            return self.ask_s3_keys(S3Next::Save { name, uri, replace });
        }
        let user = parsed.as_ref().ok().and_then(user_of);
        let Some(user) = user else {
            let purpose = InputPurpose::ConnectionUser { name, uri, replace, ask, secret };
            let dialog = InputDialog::new("Login", "User name for this server:", "", purpose);
            self.overlay = Some(Overlay::Input(dialog));
            return;
        };
        let host = parsed.as_ref().map(host_of).unwrap_or_default();
        if ask {
            if !secret.0.is_empty() {
                netrc::set_session(&host, &user, &secret.0);
            }
            return self.store_connection(name, uri, replace, true);
        }
        if secret.0.is_empty() {
            let mut dialog = InputDialog::new(
                "Login",
                &format!("Password of {user} (saved in the system keyring):"),
                "",
                InputPurpose::ConnectionPassword { name, uri, replace },
            );
            dialog.secret = true;
            self.overlay = Some(Overlay::Input(dialog));
        } else {
            self.save_login(name, uri, replace, &secret.0);
        }
    }

    /// Asks for the access key ID, then the secret access key.
    fn ask_s3_keys(&mut self, next: S3Next) {
        let dialog =
            InputDialog::new("S3 keys", "Access key ID:", "", InputPurpose::S3AccessKey { next });
        self.overlay = Some(Overlay::Input(dialog));
    }

    /// An answer of the login questions typed in a dialog.
    pub(super) fn submit_login(
        &mut self,
        purpose: InputPurpose,
        text: String,
        dialog: InputDialog,
    ) {
        match purpose {
            InputPurpose::ConnectionUser { name, uri, replace, ask, secret } => {
                let user = text.trim().to_owned();
                let with = VfsPath::parse(&uri).ok().and_then(|p| with_user(&p, &user));
                match with {
                    Some(path) if valid_user(&user) => {
                        self.login_step(name, path.to_string(), replace, ask, secret)
                    }
                    _ => {
                        let purpose =
                            InputPurpose::ConnectionUser { name, uri, replace, ask, secret };
                        self.ask_again(
                            InputDialog { purpose, ..dialog },
                            "A user name has no spaces, @ or :".to_owned(),
                        )
                    }
                }
            }
            InputPurpose::ConnectionPassword { name, uri, replace } => {
                if text.is_empty() {
                    let purpose = InputPurpose::ConnectionPassword { name, uri, replace };
                    return self.ask_again(
                        InputDialog { purpose, ..dialog },
                        "Type the password".to_owned(),
                    );
                }
                self.save_login(name, uri, replace, &text);
            }
            InputPurpose::S3AccessKey { next } => {
                let access = text.trim().to_owned();
                if access.is_empty() {
                    let purpose = InputPurpose::S3AccessKey { next };
                    return self.ask_again(
                        InputDialog { purpose, ..dialog },
                        "Type the access key ID".to_owned(),
                    );
                }
                let mut ask = InputDialog::new(
                    "S3 keys",
                    "Secret access key (hidden):",
                    "",
                    InputPurpose::S3Secret { next, access },
                );
                ask.secret = true;
                self.overlay = Some(Overlay::Input(ask));
            }
            InputPurpose::S3Secret { next, access } => {
                if text.is_empty() {
                    let purpose = InputPurpose::S3Secret { next, access };
                    return self.ask_again(
                        InputDialog { purpose, ..dialog },
                        "Type the secret access key".to_owned(),
                    );
                }
                self.use_s3_keys(next, &access, &text);
            }
            InputPurpose::ConnectionOpen { uri, test } => {
                let Ok(path) = VfsPath::parse(&uri) else { return };
                if text.is_empty() {
                    let purpose = InputPurpose::ConnectionOpen { uri, test };
                    return self.ask_again(
                        InputDialog { purpose, ..dialog },
                        "Type the password".to_owned(),
                    );
                }
                netrc::set_session(&host_of(&path), &user_of(&path).unwrap_or_default(), &text);
                self.context.registry.forget(&path);
                if test {
                    self.test_connection(uri, None);
                } else {
                    let request = self.workspace.active_mut().navigate(path);
                    self.load(request);
                }
            }
            _ => {}
        }
    }

    /// Uses the S3 keys for what they were asked for.
    fn use_s3_keys(&mut self, next: S3Next, access: &str, secret: &str) {
        match next {
            S3Next::Open { uri, test } => {
                let Ok(path) = VfsPath::parse(&uri) else { return };
                nmc_s3::settings::set_session(profile_of(&path).as_deref(), access, secret);
                self.context.registry.forget(&path);
                if test {
                    self.test_connection(uri, None);
                } else {
                    let request = self.workspace.active_mut().navigate(path);
                    self.load(request);
                }
            }
            S3Next::Save { name, uri, replace } => {
                let Ok(path) = VfsPath::parse(&uri) else { return };
                let profile = profile_of(&path);
                match nmc_s3::settings::save(profile.as_deref(), access, secret) {
                    Ok(file) => {
                        nmc_s3::settings::set_session(profile.as_deref(), access, secret);
                        self.store_connection(name, uri, replace, false);
                        self.set_status(format!("S3 keys saved in {}", file.display()));
                    }
                    Err(e) => self.set_error(format!("Cannot save the keys: {e}")),
                }
            }
        }
    }

    /// Saves the login in the system keyring and stores the connection.
    fn save_login(&mut self, name: String, uri: String, replace: Replace, password: &str) {
        let Ok(path) = VfsPath::parse(&uri) else { return };
        let (host, user) = (host_of(&path), user_of(&path).unwrap_or_default());
        match keyring::store(&host, &user, password) {
            Ok(()) => {
                netrc::set_session(&host, &user, password);
                self.store_connection(name, uri, replace, false);
                self.set_status(format!(
                    "Login of {user}@{host} saved in {}",
                    keyring::store_name()
                ));
            }
            Err(e) => self.set_error(format!(
                "Cannot save the login in {}: {e}. Choose \"ask every time\" instead",
                keyring::store_name()
            )),
        }
    }

    /// Whether the saved connection at `uri` asks for its password.
    pub(super) fn connection_asks(&self, uri: &str) -> bool {
        self.context
            .config
            .connections
            .iter()
            .any(|c| c.uri == uri && c.options.get("password").is_some_and(|v| v == "ask"))
    }

    /// Stores the connection (a new one, or the edited one) and opens a new
    /// one in the active panel.
    pub(super) fn store_connection(
        &mut self,
        name: String,
        uri: String,
        replace: Replace,
        ask: bool,
    ) {
        let set = |options: &mut std::collections::BTreeMap<String, String>| {
            if ask {
                options.insert("password".into(), "ask".into());
            } else {
                options.remove("password");
            }
        };
        match replace {
            Some((old_name, old_uri)) => {
                let Some(c) = self
                    .context
                    .config
                    .connections
                    .iter_mut()
                    .find(|c| c.name == old_name && c.uri == old_uri)
                else {
                    return;
                };
                c.name = name.clone();
                c.uri = uri;
                set(&mut c.options);
                if self.save_settings() {
                    self.set_status(format!("Saved the changes to {name}"));
                }
            }
            None => {
                let mut connection = nmc_config::Connection {
                    name: name.clone(),
                    uri: uri.clone(),
                    identity_file: None,
                    options: Default::default(),
                };
                set(&mut connection.options);
                // An older connection of this name must not pass its numbers on.
                self.manager.forget_connection_stats(&name);
                self.context.config.connections.push(connection);
                if !self.save_settings() {
                    return;
                }
                self.set_status(format!("Saved {name} in the drive list"));
                match VfsPath::parse(&uri) {
                    Ok(path) => {
                        let request = self.workspace.active_mut().navigate(path);
                        self.load(request);
                    }
                    Err(e) => self.set_error(e.to_string()),
                }
            }
        }
    }

    /// Opens (or tests) `path`; when its saved connection asks for the
    /// password every time and none was typed yet, asks for it first.
    pub(super) fn open_connection(&mut self, uri: &str, path: VfsPath, test: bool) {
        if self.connection_asks(uri)
            && uses_keys(&path)
            && !nmc_s3::settings::has_session(profile_of(&path).as_deref())
        {
            return self.ask_s3_keys(S3Next::Open { uri: uri.to_owned(), test });
        }
        let user = user_of(&path);
        let needs = self.connection_asks(uri)
            && uses_password(&path)
            && !uses_keys(&path)
            && user.as_ref().is_some_and(|u| !netrc::has_session(&host_of(&path), u));
        if needs {
            let mut dialog = InputDialog::new(
                "Password",
                &format!(
                    "Password of {}@{} (kept in memory until you quit):",
                    user.unwrap_or_default(),
                    host_of(&path)
                ),
                "",
                InputPurpose::ConnectionOpen { uri: uri.to_owned(), test },
            );
            dialog.secret = true;
            self.overlay = Some(Overlay::Input(dialog));
        } else if test {
            self.test_connection(uri.to_owned(), None);
        } else {
            let request = self.workspace.active_mut().navigate(path);
            self.load(request);
        }
    }

    /// Ctrl+T in a connection question: tests what was typed so far and
    /// comes back to the question.
    pub(super) fn test_typed(&mut self, dialog: InputDialog) {
        let typed = dialog.field.text().to_owned();
        let (location, secret) = match &dialog.purpose {
            InputPurpose::NewConnection | InputPurpose::EditConnection { .. } => {
                match check(&typed) {
                    Ok(c) => (c.path.to_string(), c.secret.unwrap_or_default()),
                    Err(e) => return self.ask_again(dialog, e),
                }
            }
            InputPurpose::NameConnection { uri, secret }
            | InputPurpose::EditConnectionName { new_uri: uri, secret, .. }
            | InputPurpose::ConnectionUser { uri, secret, .. } => (uri.clone(), secret.0.clone()),
            // The password being typed is tried too.
            InputPurpose::ConnectionPassword { uri, .. } => (uri.clone(), typed),
            InputPurpose::S3AccessKey { next: S3Next::Save { uri, .. } } => {
                (uri.clone(), String::new())
            }
            InputPurpose::S3Secret { next: S3Next::Save { uri, .. }, access } => {
                if !typed.is_empty()
                    && let Ok(path) = VfsPath::parse(uri)
                {
                    nmc_s3::settings::set_session(profile_of(&path).as_deref(), access, &typed);
                    self.context.registry.forget(&path);
                }
                (uri.clone(), String::new())
            }
            _ => return,
        };
        self.use_password_for_test(&location, &secret);
        self.test_connection(location, Some(dialog));
    }

    /// Ctrl+T in the login list: tests the connection as it stands.
    pub(super) fn test_login(&mut self, dialog: ChooseDialog) {
        let ChoosePurpose::ConnectionLogin { uri, secret, .. } = &dialog.purpose else { return };
        let (uri, secret) = (uri.clone(), secret.0.clone());
        self.use_password_for_test(&uri, &secret);
        self.test_connection_back(uri, None, Some(Box::new(dialog)));
    }

    /// A password typed for a test is used for this session only.
    fn use_password_for_test(&mut self, location: &str, secret: &str) {
        if secret.is_empty() {
            return;
        }
        if let Ok(path) = VfsPath::parse(location)
            && let Some(user) = user_of(&path)
        {
            netrc::set_session(&host_of(&path), &user, secret);
            self.context.registry.forget(&path);
        }
    }
}
