//! What a script may touch.
//!
//! Nothing by default. The user grants locations for reading or for
//! writing (which includes reading), or everything; every operation checks
//! the locations it uses before doing anything. A location inside an
//! archive counts as its archive file, so allowing a folder allows reading
//! the archives in it.

use pnxr_vfs::VfsPath;

/// How an operation uses a location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Listing, details, reading.
    Read,
    /// Creating, changing or removing.
    Write,
}

/// The locations a script may use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Permissions {
    read: Vec<VfsPath>,
    write: Vec<VfsPath>,
    run: Vec<String>,
    all: bool,
}

impl Permissions {
    /// No access at all.
    pub fn none() -> Self {
        Permissions::default()
    }

    /// Access to everything the user can reach.
    pub fn all() -> Self {
        Permissions { all: true, ..Permissions::default() }
    }

    /// Allows reading `location` and everything below it.
    pub fn allow_read(&mut self, location: VfsPath) -> &mut Self {
        self.read.push(location);
        self
    }

    /// Allows reading and changing `location` and everything below it.
    pub fn allow_write(&mut self, location: VfsPath) -> &mut Self {
        self.write.push(location);
        self
    }

    /// Allows starting the program called `name` (a bare name looked up on
    /// the PATH, never a path). `all` does not include programs.
    pub fn allow_run(&mut self, name: &str) -> &mut Self {
        self.run.push(name.to_owned());
        self
    }

    /// Checks that the script may start `program`.
    pub fn check_run(&self, program: &str) -> Result<(), String> {
        if program.is_empty() || program.contains(['/', '\\']) {
            return Err(format!(
                "{program:?}: a program is named without a folder (it is looked up on the PATH)"
            ));
        }
        if self.run.iter().any(|allowed| allowed == program) {
            Ok(())
        } else {
            Err(format!("{program}: the plugin has no permission to run this program"))
        }
    }

    /// Checks `path` for `access`; the error names the missing permission.
    pub fn check(&self, path: &VfsPath, access: Access) -> Result<(), String> {
        if self.all {
            return Ok(());
        }
        let target = outermost(path);
        let inside = |granted: &Vec<VfsPath>| granted.iter().any(|base| target.starts_with(base));
        let allowed = match access {
            Access::Read => inside(&self.read) || inside(&self.write),
            // Archives are read-only, so a path inside one is never written.
            Access::Write => path.container().is_none() && inside(&self.write),
        };
        if allowed {
            Ok(())
        } else {
            let flag = if access == Access::Read { "--allow-read" } else { "--allow-write" };
            Err(format!("{path}: the script has no permission for this location (pnxr run {flag})"))
        }
    }
}

/// The archive file holding `path` (through any nesting), or `path` itself.
fn outermost(path: &VfsPath) -> &VfsPath {
    match path.container() {
        Some(container) => outermost(container),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(text: &str) -> VfsPath {
        VfsPath::parse(text).unwrap()
    }

    #[test]
    fn nothing_is_allowed_by_default() {
        let none = Permissions::none();
        assert!(none.check(&path("/tmp/x"), Access::Read).is_err());
        assert!(Permissions::all().check(&path("sftp://h/etc"), Access::Write).is_ok());
    }

    #[test]
    fn grants_cover_what_is_below() {
        let mut p = Permissions::none();
        p.allow_read(path("/data/in")).allow_write(path("sftp://host/out"));
        assert!(p.check(&path("/data/in/a/b"), Access::Read).is_ok());
        assert!(p.check(&path("/data/in/a"), Access::Write).is_err());
        assert!(p.check(&path("/data/inside"), Access::Read).is_err());
        assert!(p.check(&path("/data/in/../secret"), Access::Read).is_err(), "normalized first");
        assert!(p.check(&path("sftp://host/out/new"), Access::Write).is_ok());
        assert!(p.check(&path("sftp://host/out/new"), Access::Read).is_ok());
        assert!(p.check(&path("sftp://other/out"), Access::Read).is_err());
        let error = p.check(&path("/etc"), Access::Write).unwrap_err();
        assert!(error.contains("--allow-write"), "{error}");
    }

    #[test]
    fn only_named_programs_run() {
        let mut p = Permissions::none();
        p.allow_run("bsdtar");
        assert!(p.check_run("bsdtar").is_ok());
        assert!(p.check_run("7z").is_err());
        assert!(p.check_run("/usr/bin/bsdtar").is_err(), "no paths");
        assert!(p.check_run("").is_err());
        assert!(Permissions::all().check_run("bsdtar").is_err(), "all is files only");
    }

    #[test]
    fn archives_count_as_their_file() {
        let mut p = Permissions::none();
        p.allow_write(path("/data"));
        let inside = path("zip:///data/a.zip!/docs/readme");
        assert!(p.check(&inside, Access::Read).is_ok());
        assert!(p.check(&inside, Access::Write).is_err(), "archives are read-only");
        assert!(p.check(&path("zip:///other/a.zip!/x"), Access::Read).is_err());
    }
}
