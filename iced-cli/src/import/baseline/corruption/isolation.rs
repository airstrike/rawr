//! Parent-owned Cargo-home identity for destructive fixture mutations.

use std::fs::{self, File};
use std::os::unix::fs::{DirBuilderExt as _, MetadataExt as _};
use std::path::{Path, PathBuf};

use rawr_import::{cargo, context::Context};

/// Exclusive creation and retained directory identity, never a path-prefix claim.
pub(in crate::import::baseline) struct Home {
    root: PathBuf,
    parent: PathBuf,
    parent_identity: Identity,
    identity: Identity,
    held: File,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
struct Identity {
    device: u64,
    inode: u64,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::import::baseline) enum Error {
    #[error("isolated Cargo home I/O failed at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("isolated Cargo home directory identity changed at {0}")]
    Changed(PathBuf),
    #[error("isolated Cargo home path is not Unicode")]
    Encoding,
    #[error(transparent)]
    Import(#[from] rawr_import::Error),
    #[error(transparent)]
    Context(#[from] rawr_import::context::Error),
}

impl Home {
    /// Create an empty home under a root already exclusively owned by the fixture.
    /// Registry/git seeding must be explicit; there is no ambient-store fallback.
    pub(in crate::import::baseline) fn create(parent: &Path) -> Result<Self, Error> {
        let parent_identity = directory(parent)?;
        let root = parent.join("baseline-cargo-home");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|source| io(&root, source))?;
        let identity = directory(&root)?;
        let held = File::open(&root).map_err(|source| io(&root, source))?;
        let home = Self {
            root,
            parent: parent.into(),
            parent_identity,
            identity,
            held,
        };
        home.verify()?;
        Ok(home)
    }

    pub(in crate::import::baseline) fn verify(&self) -> Result<(), Error> {
        let held = self
            .held
            .metadata()
            .map_err(|source| io(&self.root, source))?;
        if !held.is_dir()
            || identity(&held) != self.identity
            || directory(&self.root)? != self.identity
            || directory(&self.parent)? != self.parent_identity
        {
            return Err(Error::Changed(self.root.clone()));
        }
        Ok(())
    }

    pub(in crate::import::baseline) fn configure(
        &self,
        command: &mut std::process::Command,
    ) -> Result<(), Error> {
        self.verify()?;
        command.env("CARGO_HOME", &self.root);
        Ok(())
    }

    pub(in crate::import::baseline) fn context(
        &self,
        project: &Path,
        target: &str,
    ) -> Result<Context, Error> {
        self.verify()?;
        let home = self.root.to_str().ok_or(Error::Encoding)?;
        let environment = cargo::config::Environment::capture()?.setting("CARGO_HOME", home)?;
        let context = Context::capture(project, environment, target, &cargo::Limits::standard())?;
        if context.cargo_home() != self.root {
            return Err(Error::Changed(self.root.clone()));
        }
        Ok(context)
    }

    pub(in crate::import::baseline) fn root(&self) -> &Path {
        &self.root
    }
}

fn directory(path: &Path) -> Result<Identity, Error> {
    let metadata = fs::symlink_metadata(path).map_err(|source| io(path, source))?;
    if !metadata.is_dir()
        || metadata.file_type().is_symlink()
        || fs::canonicalize(path).map_err(|source| io(path, source))? != path
    {
        return Err(Error::Changed(path.into()));
    }
    Ok(identity(&metadata))
}

fn identity(metadata: &fs::Metadata) -> Identity {
    Identity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}

fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rawr-corruption-home-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::canonicalize(root).unwrap()
    }

    #[test]
    fn home_requires_exclusive_creation_and_retains_the_original_directory() {
        let root = root();
        let home = super::Home::create(&root).unwrap();
        assert!(super::Home::create(&root).is_err());
        let moved = root.join("moved-home");
        fs::rename(home.root(), &moved).unwrap();
        fs::create_dir(home.root()).unwrap();
        assert!(matches!(home.verify(), Err(super::Error::Changed(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_symlink_to_the_retained_home_cannot_substitute_for_its_owned_path() {
        let root = root();
        let home = super::Home::create(&root).unwrap();
        let moved = root.join("moved-home");
        fs::rename(home.root(), &moved).unwrap();
        symlink(&moved, home.root()).unwrap();
        assert!(matches!(home.verify(), Err(super::Error::Changed(_))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn each_child_uses_the_exact_owned_home_binding() {
        let root = root();
        let home = super::Home::create(&root).unwrap();
        let mut command = std::process::Command::new("not-executed");
        command.env("CARGO_HOME", "/ambient/home");
        home.configure(&mut command).unwrap();
        let binding = command
            .get_envs()
            .find(|(key, _)| *key == "CARGO_HOME")
            .unwrap()
            .1
            .unwrap();
        assert_eq!(binding, home.root().as_os_str());
        fs::remove_dir_all(root).unwrap();
    }
}
