//! Validated baseline and actual-generation storage.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{generation, inventory, manifest, Commit, Error};

pub(super) struct Store {
    root: PathBuf,
    baseline: PathBuf,
    actual: PathBuf,
}

impl Store {
    pub(super) fn new(root: PathBuf) -> Self {
        Self {
            baseline: root.join("snapshots"),
            actual: root.join("target").join("rawr").join("snapshots"),
            root,
        }
    }

    pub(super) fn baseline(&self) -> &Path {
        &self.baseline
    }

    pub(super) fn actual(&self) -> &Path {
        &self.actual
    }

    pub(super) fn check(
        &self,
        generation: &generation::Generation,
    ) -> Result<generation::Outcome, Error> {
        let commit = replace(&self.root, &self.actual, generation)?;
        let compared = (|| {
            validate_entry(&self.root, &self.baseline)?;
            let inventory = inventory::Inventory::read(&self.baseline)?;
            let manifest_path = Path::new(manifest::FILE);
            let manifest = manifest::Manifest::read(
                &self.baseline.join(manifest_path),
                inventory.bytes(manifest_path),
            )?;
            generation.compare(&self.baseline, &self.actual, manifest.as_ref(), &inventory)
        })();
        let (differences, commit) = commit.follow(compared)?;
        Ok(generation::Outcome::new(
            self.baseline.clone(),
            self.actual.clone(),
            generation.len(),
            differences,
            false,
            commit,
        ))
    }

    /// Replace the actual tree with `generation` and compare nothing: one
    /// selected preview rendered on request.
    pub(super) fn actual_only(
        &self,
        generation: &generation::Generation,
    ) -> Result<generation::Outcome, Error> {
        let commit = replace(&self.root, &self.actual, generation)?;
        Ok(generation::Outcome::new(
            self.baseline.clone(),
            self.actual.clone(),
            generation.len(),
            Vec::new(),
            false,
            commit,
        ))
    }

    pub(super) fn update(
        &self,
        generation: &generation::Generation,
    ) -> Result<generation::Outcome, Error> {
        let actual = replace(&self.root, &self.actual, generation)?;
        let (baseline, actual) = actual.follow(replace(&self.root, &self.baseline, generation))?;
        let commit = actual.merge(baseline);
        Ok(generation::Outcome::new(
            self.baseline.clone(),
            self.actual.clone(),
            generation.len(),
            Vec::new(),
            true,
            commit,
        ))
    }
}

fn replace(
    root: &Path,
    target: &Path,
    generation: &generation::Generation,
) -> Result<Commit, Error> {
    let parent = target.parent().ok_or_else(|| Error::UnsafePath {
        path: target.to_path_buf(),
    })?;
    validate(root, parent)?;
    fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    validate_entry(root, target)?;

    let stage = reserve(parent, "stage")?;
    if let Err(operation) = generation.write(&stage) {
        return Err(cleanup(target, stage, operation));
    }

    install(target, stage)
}

fn install(target: &Path, stage: PathBuf) -> Result<Commit, Error> {
    install_with(
        target,
        stage,
        |from, to| fs::rename(from, to),
        |path| fs::remove_dir_all(path),
    )
}

fn install_with(
    target: &Path,
    stage: PathBuf,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
    mut discard: impl FnMut(&Path) -> std::io::Result<()>,
) -> Result<Commit, Error> {
    let parent = target.parent().ok_or_else(|| Error::UnsafePath {
        path: target.to_path_buf(),
    })?;
    let mut backup = None;
    let present = match fs::symlink_metadata(target) {
        Ok(metadata) => match directory(target, &metadata) {
            Ok(()) => true,
            Err(operation) => return Err(cleanup(target, stage, operation)),
        },
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => false,
        Err(source) => return Err(cleanup(target, stage, Error::io(target, source))),
    };
    if present {
        let path = match reserve(parent, "backup") {
            Ok(path) => path,
            Err(operation) => return Err(cleanup(target, stage, operation)),
        };
        if let Err(source) = fs::remove_dir(&path) {
            return Err(cleanup(target, stage, Error::io(&path, source)));
        }
        if let Err(source) = rename(target, &path) {
            return Err(cleanup(target, stage, Error::io(target, source)));
        }
        backup = Some(path);
    }

    if let Err(source) = rename(&stage, target) {
        if let Some(backup) = backup {
            return match rename(&backup, target) {
                Ok(()) => Err(cleanup(target, stage, Error::io(target, source))),
                Err(rollback) => Err(Error::Rollback {
                    target: target.to_path_buf(),
                    backup,
                    stage,
                    source,
                    rollback,
                }),
            };
        }
        return Err(cleanup(target, stage, Error::io(target, source)));
    }

    let mut commit = Commit::new();
    if let Some(backup) = backup
        && let Err(source) = discard(&backup)
    {
        commit.warn(target, backup, source);
    }
    Ok(commit)
}

fn validate(root: &Path, parent: &Path) -> Result<(), Error> {
    let relative = parent.strip_prefix(root).map_err(|_| Error::UnsafePath {
        path: parent.to_path_buf(),
    })?;
    if relative
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(Error::UnsafePath {
            path: parent.to_path_buf(),
        });
    }
    let root_metadata = fs::symlink_metadata(root).map_err(|source| Error::io(root, source))?;
    directory(root, &root_metadata)?;
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => directory(&path, &metadata)?,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => return Err(Error::io(&path, source)),
        }
    }
    Ok(())
}

fn validate_entry(root: &Path, target: &Path) -> Result<(), Error> {
    let parent = target.parent().ok_or_else(|| Error::UnsafePath {
        path: target.to_path_buf(),
    })?;
    validate(root, parent)?;
    match fs::symlink_metadata(target) {
        Ok(metadata) => directory(target, &metadata),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(Error::io(target, source)),
    }
}

fn directory(path: &Path, metadata: &fs::Metadata) -> Result<(), Error> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::UnsafePath {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

fn cleanup(target: &Path, recoverable: PathBuf, operation: Error) -> Error {
    match fs::remove_dir_all(&recoverable) {
        Ok(()) => operation,
        Err(cleanup) => Error::Remains {
            target: target.to_path_buf(),
            recoverable,
            operation: Box::new(operation),
            cleanup,
        },
    }
}

fn reserve(parent: &Path, role: &str) -> Result<PathBuf, Error> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    for _ in 0..100 {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".rawr-snapshot-{role}-{}-{id}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => return Err(Error::io(path, source)),
        }
    }
    Err(Error::io(
        parent,
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not reserve a unique snapshot staging directory",
        ),
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::Error;

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[cfg(unix)]
    #[test]
    fn validation_rejects_every_existing_escape_or_non_directory_ancestor() {
        use std::os::unix::fs::symlink;

        let root = temporary("ancestor-root");
        let outside = temporary("ancestor-outside");
        symlink(&outside, root.join("target")).unwrap();
        assert_unsafe(
            super::validate(&root, &root.join("target/rawr")).unwrap_err(),
            &root.join("target"),
        );
        assert!(fs::read_dir(&outside).unwrap().next().is_none());

        fs::remove_file(root.join("target")).unwrap();
        fs::create_dir(root.join("target")).unwrap();
        symlink(&outside, root.join("target/rawr")).unwrap();
        assert_unsafe(
            super::validate(&root, &root.join("target/rawr")).unwrap_err(),
            &root.join("target/rawr"),
        );

        fs::remove_file(root.join("target/rawr")).unwrap();
        fs::remove_dir(root.join("target")).unwrap();
        fs::write(root.join("target"), b"not a directory").unwrap();
        assert_unsafe(
            super::validate(&root, &root.join("target/rawr")).unwrap_err(),
            &root.join("target"),
        );
        assert_unsafe(super::validate(&root, &outside).unwrap_err(), &outside);

        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }

    #[test]
    fn failed_install_restores_the_prior_tree_and_removes_its_stage() {
        let root = temporary("rollback");
        let target = root.join("snapshots");
        let stage = root.join("stage");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("value"), b"prior").unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("value"), b"fresh").unwrap();
        let mut calls = 0;

        let error = super::install_with(
            &target,
            stage.clone(),
            |from, to| {
                calls += 1;
                if calls == 2 {
                    Err(std::io::Error::other("injected install failure"))
                } else {
                    fs::rename(from, to)
                }
            },
            |path| fs::remove_dir_all(path),
        )
        .unwrap_err();

        assert!(matches!(error, Error::Io { .. }));
        assert_eq!(fs::read(target.join("value")).unwrap(), b"prior");
        assert!(!stage.exists());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_rollback_names_both_recoverable_generations() {
        let root = temporary("rollback-paths");
        let target = root.join("snapshots");
        let stage = root.join("stage");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("value"), b"prior").unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("value"), b"fresh").unwrap();
        let mut calls = 0;

        let error = super::install_with(
            &target,
            stage.clone(),
            |from, to| {
                calls += 1;
                if calls >= 2 {
                    Err(std::io::Error::other("injected rename failure"))
                } else {
                    fs::rename(from, to)
                }
            },
            |path| fs::remove_dir_all(path),
        )
        .unwrap_err();
        let text = error.to_string();
        let Error::Rollback {
            backup,
            stage: retained,
            ..
        } = error
        else {
            panic!("expected rollback evidence, got {text}");
        };
        assert_eq!(retained, stage);
        assert!(backup.is_dir());
        assert!(stage.is_dir());
        assert!(text.contains(&backup.display().to_string()));
        assert!(text.contains(&stage.display().to_string()));

        fs::remove_dir_all(backup).unwrap();
        fs::remove_dir_all(stage).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn post_commit_cleanup_failure_is_a_successful_notice() {
        let root = temporary("cleanup-paths");
        let target = root.join("snapshots");
        let stage = root.join("stage");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("value"), b"prior").unwrap();
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("value"), b"fresh").unwrap();

        let result = super::install_with(
            &target,
            stage,
            |from, to| fs::rename(from, to),
            |_| Err(std::io::Error::other("injected cleanup failure")),
        );
        assert!(result.is_ok(), "a committed target cannot accompany Err");
        let commit = result.unwrap();
        let [notice] = commit.notices() else {
            panic!("expected one post-commit cleanup notice");
        };
        assert_eq!(fs::read(target.join("value")).unwrap(), b"fresh");
        assert_eq!(notice.target(), target);
        assert!(notice.recoverable().is_dir());
        assert_eq!(notice.source().kind(), std::io::ErrorKind::Other);

        fs::remove_dir_all(notice.recoverable()).unwrap();
        fs::remove_dir_all(target).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pre_commit_cleanup_failure_names_its_recoverable_stage() {
        let root = temporary("cleanup-stage");
        let target = root.join("snapshots");

        let stage = root.join("stage-file");
        fs::write(&stage, b"fresh").unwrap();
        let operation = Error::UnsafePath {
            path: target.clone(),
        };
        let remains = super::cleanup(&target, stage.clone(), operation);
        assert!(matches!(remains, Error::Remains { .. }));
        assert!(remains.to_string().contains(&target.display().to_string()));
        assert!(remains.to_string().contains(&stage.display().to_string()));

        fs::remove_file(stage).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn assert_unsafe(error: Error, expected: &Path) {
        let Error::UnsafePath { path } = error else {
            panic!("expected unsafe path `{}`, got {error}", expected.display());
        };
        assert_eq!(path, expected);
    }

    fn temporary(label: &str) -> PathBuf {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("rawr-snapshot-{label}-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        fs::canonicalize(path).unwrap()
    }
}
