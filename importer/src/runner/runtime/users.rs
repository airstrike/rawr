//! The rawr executables that link a runtime entry.
//!
//! An entry's key covers rawr's sources, so only an executable built from
//! those sources asks for it again. Each one that links the entry records
//! itself in the entry's `users` file by its path, length and modification
//! time. An executable that `cargo install` or a rebuild replaced has another
//! length or time, and one that was deleted has neither, so an entry none of
//! whose users is still there unchanged is one no installed rawr reaches.
//!
//! A Cargo build's own output, such as `target/debug/rawr` or a test binary
//! in `target/debug/deps`, is a development build that nothing installed
//! runs. It is never recorded, and a record of one counts as no user: a dev
//! or test build left on disk unchanged would otherwise keep every entry it
//! ever used (#1677).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// In each entry: one line per executable, `<length> <modified ns> <path>`.
const FILE: &str = "users";

/// The executables recorded in one entry.
pub(crate) struct Users {
    file: PathBuf,
    lines: Vec<String>,
}

impl Users {
    /// The users `entry` records; none when it records none.
    pub(crate) fn of(entry: &Path) -> Self {
        let file = entry.join(FILE);
        let lines = fs::read_to_string(&file)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        Self { file, lines }
    }

    /// Whether an executable recorded here is still there unchanged.
    pub(crate) fn any_installed(&self) -> bool {
        self.lines.iter().any(|line| installed(line))
    }

    /// Record `executable` among the users, dropping users that are gone or
    /// replaced. The caller holds the entry's exclusive lock. One read and
    /// no write when it is already recorded.
    pub(crate) fn enroll(self, executable: &Path) {
        let Some(user) = line(executable) else {
            return;
        };
        if self.lines.contains(&user) {
            return;
        }
        let mut text = String::new();
        for kept in self.lines.iter().filter(|line| installed(line)) {
            text.push_str(kept);
            text.push('\n');
        }
        text.push_str(&user);
        text.push('\n');
        let _ = fs::write(&self.file, text);
    }
}

/// The users line of `executable` as it is now; none for a build's output.
fn line(executable: &Path) -> Option<String> {
    if built(executable) {
        return None;
    }
    Some(format!("{} {}", stamp(executable)?, executable.to_str()?))
}

/// Whether `executable` is a Cargo build's output: it sits in a profile
/// directory, which holds Cargo's `.fingerprint`, or in that directory's
/// `deps` or `examples`.
fn built(executable: &Path) -> bool {
    let Some(parent) = executable.parent() else {
        return false;
    };
    let profile = match parent.file_name().and_then(OsStr::to_str) {
        Some("deps" | "examples") => parent.parent(),
        _ => Some(parent),
    };
    profile.is_some_and(|profile| profile.join(".fingerprint").is_dir())
}

/// An executable's length and modification time.
fn stamp(executable: &Path) -> Option<String> {
    let metadata = fs::metadata(executable).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!("{} {}", metadata.len(), modified.as_nanos()))
}

/// Whether a users line names an installed executable still there
/// unchanged.
fn installed(recorded: &str) -> bool {
    let mut parts = recorded.splitn(3, ' ');
    let (Some(length), Some(modified), Some(path)) = (parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let path = Path::new(path);
    !built(path) && stamp(path).is_some_and(|now| now == format!("{length} {modified}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, SystemTime};

    #[test]
    fn an_entry_is_reached_while_a_recorded_executable_is_unchanged() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-runtime-users-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let entry = root.join("entry");
        fs::create_dir_all(&entry).unwrap();
        let old = root.join("old-rawr");
        let new = root.join("new-rawr");
        fs::write(&old, b"old").unwrap();
        fs::write(&new, b"new").unwrap();

        assert!(!super::Users::of(&entry).any_installed(), "no users yet");
        super::Users::of(&entry).enroll(&old);
        super::Users::of(&entry).enroll(&old);
        assert_eq!(
            fs::read_to_string(entry.join(super::FILE))
                .unwrap()
                .lines()
                .count(),
            1
        );
        assert!(super::Users::of(&entry).any_installed());

        // A reinstall replaces the executable at the same path.
        fs::write(&old, b"reinstalled").unwrap();
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(SystemTime::now() + Duration::from_secs(5))
            .unwrap();
        assert!(
            !super::Users::of(&entry).any_installed(),
            "the user was replaced"
        );

        super::Users::of(&entry).enroll(&new);
        let recorded = fs::read_to_string(entry.join(super::FILE)).unwrap();
        assert_eq!(
            recorded.lines().count(),
            1,
            "the replaced user is dropped: {recorded}"
        );
        assert!(super::Users::of(&entry).any_installed());
        fs::remove_file(&new).unwrap();
        assert!(
            !super::Users::of(&entry).any_installed(),
            "the user was deleted"
        );
        fs::remove_dir_all(root).unwrap();
    }

    /// A Cargo build's output is no user: a dev build in a profile
    /// directory and a test binary in its `deps` are never recorded, and a
    /// record an earlier rawr made of one reaches nothing (#1677).
    #[test]
    fn a_build_output_is_no_user() {
        let root =
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
                .join(format!("rawr-runtime-built-users-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let profile = root.join("target/debug");
        fs::create_dir_all(profile.join(".fingerprint")).unwrap();
        fs::create_dir_all(profile.join("deps")).unwrap();
        let development = profile.join("rawr-import");
        let test = profile.join("deps/rawr_import-0123456789abcdef");
        let installed = root.join("bin/rawr-import");
        fs::create_dir_all(installed.parent().unwrap()).unwrap();
        for executable in [&development, &test, &installed] {
            fs::write(executable, b"rawr").unwrap();
        }
        let entry = root.join("entry");
        fs::create_dir_all(&entry).unwrap();
        super::Users::of(&entry).enroll(&development);
        super::Users::of(&entry).enroll(&test);
        assert!(
            !entry.join(super::FILE).exists(),
            "no build output is recorded"
        );
        assert!(!super::Users::of(&entry).any_installed());

        // Records an earlier rawr made of both, beside an installed rawr's.
        let recorded = [&development, &test]
            .map(|executable| {
                format!(
                    "{} {}",
                    super::stamp(executable).unwrap(),
                    executable.display()
                )
            })
            .join("\n");
        fs::write(entry.join(super::FILE), format!("{recorded}\n")).unwrap();
        assert!(!super::Users::of(&entry).any_installed(), "{recorded}");
        super::Users::of(&entry).enroll(&installed);
        let users = fs::read_to_string(entry.join(super::FILE)).unwrap();
        assert_eq!(users.lines().count(), 1, "the build outputs go: {users}");
        assert!(super::Users::of(&entry).any_installed());
        fs::remove_dir_all(root).unwrap();
    }
}
