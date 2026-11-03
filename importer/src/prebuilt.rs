//! rawr's prebuilt binaries, one archive per host on each version's GitHub
//! release. The release workflow (`.github/workflows/binaries.yml`) and the
//! installers (`site/public/install.sh`, `install.ps1`) name the archives the
//! same way.
//!
//! A `rawr-import` built from source without its driver fetches the driver
//! from the archive of its own version and host. That driver is the pinned
//! nightly's, the one a project that names no toolchain reads with; a
//! project on another nightly gets a driver built for it once
//! (`cargo/toolchain/driver.rs`).

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest as _, Sha256};

use crate::{Error, Result};

/// The repository whose releases carry the archives.
const RELEASES: &str = "https://github.com/airstrike/rawr/releases/download";

/// The command that installs rawr's prebuilt binaries.
pub const INSTALL_COMMAND: &str = "curl -LsSf https://rawr.airstrike.fyi/install.sh | sh";

/// One host's archive of one version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Archive {
    name: String,
    url: String,
}

impl Archive {
    /// The archive of `version` for the host `triple`. Windows archives are
    /// zip files, every other host's a gzipped tarball.
    pub fn new(triple: &str, version: &str) -> Self {
        let extension = if triple.contains("-windows-") {
            "zip"
        } else {
            "tar.gz"
        };
        let name = format!("rawr-{triple}.{extension}");
        let url = format!("{RELEASES}/v{version}/{name}");
        Self { name, url }
    }

    /// The archive of this build's version for its host.
    pub fn current() -> Self {
        Self::new(crate::cargo::toolchain::HOST, env!("CARGO_PKG_VERSION"))
    }

    /// The file name, the same for every version.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Where the release keeps it.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Where the release keeps its SHA-256, as `sha256sum` prints it.
    pub fn checksum_url(&self) -> String {
        format!("{}.sha256", self.url)
    }

    /// Download this archive and install `binary` from it into `directory`.
    /// It uses `curl` and `tar`, which macOS, Linux and Windows 10 ship.
    pub fn install(&self, binary: &str, directory: &Path) -> Result<PathBuf> {
        let staging = directory.join(format!(".{binary}.download-{}", std::process::id()));
        let result = self.install_through(binary, directory, &staging);
        let _ = fs::remove_dir_all(&staging);
        result.map_err(|reason| Error::Request {
            reason: format!(
                "could not install `{binary}` from {}: {reason}. Install rawr's prebuilt \
                 binaries with\n  {INSTALL_COMMAND}",
                self.url
            )
            .into_boxed_str(),
        })
    }

    fn install_through(
        &self,
        binary: &str,
        directory: &Path,
        staging: &Path,
    ) -> std::result::Result<PathBuf, String> {
        fs::create_dir_all(staging).map_err(|error| error.to_string())?;
        let archive = staging.join(&self.name);
        let checksum = staging.join(format!("{}.sha256", self.name));
        run(
            "curl",
            [
                OsStr::new("-fsSL"),
                OsStr::new("-o"),
                archive.as_os_str(),
                OsStr::new(&self.url),
            ],
        )?;
        let checksum_url = self.checksum_url();
        run(
            "curl",
            [
                OsStr::new("-fsSL"),
                OsStr::new("-o"),
                checksum.as_os_str(),
                OsStr::new(&checksum_url),
            ],
        )?;
        let expected = fs::read_to_string(&checksum).map_err(|error| error.to_string())?;
        let bytes = fs::read(&archive).map_err(|error| error.to_string())?;
        if !Checksum::of(&bytes).matches(&expected) {
            return Err("the download does not match its published SHA-256".into());
        }
        let file = format!("{binary}{}", std::env::consts::EXE_SUFFIX);
        run(
            "tar",
            [
                OsStr::new("-xf"),
                archive.as_os_str(),
                OsStr::new("-C"),
                staging.as_os_str(),
                OsStr::new(&file),
            ],
        )?;
        let installed = directory.join(&file);
        fs::rename(staging.join(&file), &installed).map_err(|error| error.to_string())?;
        Ok(installed)
    }
}

/// A SHA-256 digest, in lowercase hex.
struct Checksum(String);

impl Checksum {
    fn of(bytes: &[u8]) -> Self {
        let digest = Sha256::digest(bytes);
        Self(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }

    /// Whether a `sha256sum` line, digest first, names this digest.
    fn matches(&self, line: &str) -> bool {
        line.split_ascii_whitespace()
            .next()
            .is_some_and(|digest| digest.eq_ignore_ascii_case(&self.0))
    }
}

fn run<'a>(
    program: &str,
    arguments: impl IntoIterator<Item = &'a OsStr>,
) -> std::result::Result<(), String> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("`{program}` could not run: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "`{program}` failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{Archive, Checksum};

    #[test]
    fn each_host_names_its_archive() {
        for (triple, name) in [
            ("aarch64-apple-darwin", "rawr-aarch64-apple-darwin.tar.gz"),
            ("x86_64-apple-darwin", "rawr-x86_64-apple-darwin.tar.gz"),
            (
                "x86_64-unknown-linux-gnu",
                "rawr-x86_64-unknown-linux-gnu.tar.gz",
            ),
            (
                "aarch64-unknown-linux-gnu",
                "rawr-aarch64-unknown-linux-gnu.tar.gz",
            ),
            ("x86_64-pc-windows-msvc", "rawr-x86_64-pc-windows-msvc.zip"),
        ] {
            assert_eq!(Archive::new(triple, "0.3.115").name(), name);
        }
    }

    #[test]
    fn an_archive_lives_on_its_version_release() {
        let archive = Archive::new("aarch64-apple-darwin", "0.3.115");
        assert_eq!(
            archive.url(),
            "https://github.com/airstrike/rawr/releases/download/v0.3.115/rawr-aarch64-apple-darwin.tar.gz"
        );
        assert_eq!(
            archive.checksum_url(),
            "https://github.com/airstrike/rawr/releases/download/v0.3.115/rawr-aarch64-apple-darwin.tar.gz.sha256"
        );
    }

    #[test]
    fn this_build_names_its_own_host_and_version() {
        let archive = Archive::current();
        assert!(archive.name().contains(crate::cargo::toolchain::HOST));
        assert!(archive
            .url()
            .contains(concat!("/v", env!("CARGO_PKG_VERSION"), "/")));
    }

    #[test]
    fn a_checksum_matches_a_sha256sum_line() {
        let checksum = Checksum::of(b"abc");
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(checksum.matches(&format!("{digest}  rawr-aarch64-apple-darwin.tar.gz\n")));
        assert!(checksum.matches(&digest.to_uppercase()));
        assert!(!checksum.matches("00  rawr.tar.gz"));
        assert!(!checksum.matches(""));
    }

    #[cfg(unix)]
    #[test]
    fn install_takes_one_binary_from_a_checked_archive() {
        use std::fs;
        use std::process::Command;

        let root = std::env::temp_dir().join(format!("rawr-prebuilt-{}", std::process::id()));
        let release = root.join("release");
        let installed = root.join("bin");
        fs::create_dir_all(&release).unwrap();
        fs::create_dir_all(&installed).unwrap();
        fs::write(release.join("rawr-import-driver"), b"driver").unwrap();
        fs::write(release.join("rawr"), b"rawr").unwrap();
        let name = "rawr-aarch64-apple-darwin.tar.gz";
        let status = Command::new("tar")
            .args(["-czf", name, "rawr", "rawr-import-driver"])
            .current_dir(&release)
            .status()
            .unwrap();
        assert!(status.success());
        let bytes = fs::read(release.join(name)).unwrap();
        fs::write(
            release.join(format!("{name}.sha256")),
            format!("{}  {name}\n", Checksum::of(&bytes).0),
        )
        .unwrap();
        let archive = Archive {
            name: name.into(),
            url: format!("file://{}/{name}", release.display()),
        };

        let driver = archive.install("rawr-import-driver", &installed).unwrap();
        assert_eq!(fs::read(&driver).unwrap(), b"driver");
        assert!(!installed.join("rawr").exists());
        assert_eq!(fs::read_dir(&installed).unwrap().count(), 1);

        fs::write(release.join(format!("{name}.sha256")), "00  x\n").unwrap();
        assert!(archive.install("rawr-import-driver", &installed).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
