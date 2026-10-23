//! Raw durable-state proof for one release launch, outside its visibility clock.

use std::fs;
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use rawr_import::{cargo, context::Context, metrics::cell::project};

#[derive(Debug, serde::Serialize)]
pub(super) struct Before {
    project: PathBuf,
    target: Box<str>,
    context: [u8; 32],
    local: Captured,
    shared: Captured,
}

#[derive(Debug, serde::Serialize)]
struct Captured {
    root: Root,
    manifest: project::Manifest,
}

/// A path and physical directory association, including explicit absence.
#[derive(Debug, Eq, PartialEq, serde::Serialize)]
struct Root {
    path: Box<[u8]>,
    identity: Option<[u64; 2]>,
}

#[derive(Debug, serde::Serialize)]
struct Transition {
    before_root: Root,
    after_root: Root,
    raw: project::Transition,
}

/// Reached evidence remains available on a durable-state refusal.
#[derive(Debug, serde::Serialize)]
pub(super) struct Observed {
    context_before: [u8; 32],
    context_after: [u8; 32],
    project: Transition,
    shared: Transition,
}

/// Only comparison of complete raw manifests can construct this proof.
#[derive(Debug, serde::Serialize)]
pub(super) struct Unchanged(Observed);

#[derive(Debug, thiserror::Error)]
pub(super) enum Error {
    #[error(transparent)]
    Import(#[from] rawr_import::Error),
    #[error(transparent)]
    Context(#[from] rawr_import::context::Error),
    #[error(transparent)]
    Manifest(#[from] rawr_import::metrics::cell::Error),
    #[error("release cache path {path} cannot be observed: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("release cache root has an invalid shape or association: {0}")]
    Root(PathBuf),
    #[error("release launch changed its context, project cache, or shared store")]
    Changed(Box<Observed>),
    #[error("release post-launch cache observation failed: {source}")]
    Incomplete {
        #[source]
        source: Box<Error>,
        before: Box<Before>,
    },
}

impl Before {
    pub(super) fn capture(project: &Path, target: &str) -> Result<Self, Error> {
        Self::capture_in(project, target, cargo::config::Environment::capture()?)
    }

    fn capture_in(
        project: &Path,
        target: &str,
        environment: cargo::config::Environment,
    ) -> Result<Self, Error> {
        let context = Context::capture(project, environment, target, &cargo::Limits::standard())?;
        let local = Captured::read(&context.project().join("target/rawr/cache"), false)?;
        let shared = shared(&context)?;
        Ok(Self {
            project: context.project().into(),
            target: target.into(),
            context: context.identity(),
            local,
            shared,
        })
    }

    pub(super) fn finish(self) -> Result<Unchanged, Error> {
        match cargo::config::Environment::capture() {
            Ok(environment) => self.finish_in(environment),
            Err(source) => Err(Error::Incomplete {
                source: Box::new(source.into()),
                before: Box::new(self),
            }),
        }
    }

    fn finish_in(self, environment: cargo::config::Environment) -> Result<Unchanged, Error> {
        let after = match Self::capture_in(&self.project, &self.target, environment) {
            Ok(after) => after,
            Err(source) => {
                return Err(Error::Incomplete {
                    source: Box::new(source),
                    before: Box::new(self),
                })
            }
        };
        let observed = Observed {
            context_before: self.context,
            context_after: after.context,
            project: Transition::compare(self.local, after.local),
            shared: Transition::compare(self.shared, after.shared),
        };
        if observed.context_before != observed.context_after
            || !observed.project.unchanged()
            || !observed.shared.unchanged()
        {
            return Err(Error::Changed(Box::new(observed)));
        }
        Ok(Unchanged(observed))
    }
}

impl Unchanged {
    /// Each value already proves its own endpoints equal. Comparing endpoints
    /// also detects durable changes between two individually warm launches.
    pub(super) fn same_state(&self, other: &Self) -> bool {
        self.0.context_after == other.0.context_before
            && self.0.project.after_root == other.0.project.before_root
            && self.0.project.raw.after == other.0.project.raw.before
            && self.0.shared.after_root == other.0.shared.before_root
            && self.0.shared.raw.after == other.0.shared.raw.before
    }
}

impl Error {
    pub(super) fn observed(&self) -> Option<&Observed> {
        match self {
            Self::Changed(observed) => Some(observed),
            _ => None,
        }
    }

    pub(super) fn before(&self) -> Option<&Before> {
        match self {
            Self::Incomplete { before, .. } => Some(before),
            _ => None,
        }
    }
}

fn shared(context: &Context) -> Result<Captured, Error> {
    // Match Context::open_store's one production root (`<target>/rawr`)
    // without opening or creating the store. A warm series requires its setup
    // to have created it.
    let parent = context.rawr().to_path_buf();
    let association = Root::read(&parent, true)?;
    let captured = Captured::read(&parent.join("store"), true)?;
    if association != Root::read(&parent, true)? {
        return Err(Error::Root(parent));
    }
    Ok(captured)
}

impl Captured {
    fn read(path: &Path, required: bool) -> Result<Self, Error> {
        // Bind the manifest's visited contents and permissions to the same
        // physical root on both sides of the walk.
        let root = Root::read(path, required)?;
        let manifest = project::Manifest::read(path)?;
        if root != Root::read(path, required)? {
            return Err(Error::Root(path.into()));
        }
        Ok(Self { root, manifest })
    }
}

impl Root {
    fn read(path: &Path, required: bool) -> Result<Self, Error> {
        if !path.is_absolute() || path.as_os_str().len() > 16 * 1024 {
            return Err(Error::Root(path.into()));
        }
        let identity = match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::Root(path.into()));
                }
                let canonical = fs::canonicalize(path).map_err(|source| Error::Io {
                    path: path.into(),
                    source,
                })?;
                if canonical != path {
                    return Err(Error::Root(path.into()));
                }
                Some([metadata.dev(), metadata.ino()])
            }
            Err(source) if !required && source.kind() == std::io::ErrorKind::NotFound => None,
            Err(source) => {
                return Err(Error::Io {
                    path: path.into(),
                    source,
                })
            }
        };
        Ok(Self {
            path: path.as_os_str().as_bytes().into(),
            identity,
        })
    }
}

impl Transition {
    fn compare(before: Captured, after: Captured) -> Self {
        Self {
            before_root: before.root,
            after_root: after.root,
            raw: project::Transition::transform(before.manifest, after.manifest),
        }
    }

    fn unchanged(&self) -> bool {
        self.before_root == self.after_root
            && self.raw.before == self.raw.after
            && self.raw.changes.is_empty()
            && self.raw.directory_changes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);

    impl Fixture {
        fn transform() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rawr-release-cache-{}-{}",
                std::process::id(),
                super::super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            let fixture = Self(fs::canonicalize(path).unwrap());
            for relative in ["project/target/rawr/cache", "cargo/rawr/v1", "other/rawr/v1"] {
                fs::create_dir_all(fixture.0.join(relative)).unwrap();
            }
            fixture
        }

        fn environment(&self, home: &str) -> cargo::config::Environment {
            cargo::config::Environment::from_pairs([(
                "CARGO_HOME",
                self.0.join(home).to_str().unwrap(),
            )])
            .unwrap()
        }

        fn capture(&self) -> Before {
            Before::capture_in(
                &self.0.join("project"),
                "fixture-target",
                self.environment("cargo"),
            )
            .unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unchanged_warm_state_binds_the_production_shared_root() {
        let fixture = Fixture::transform();
        fs::write(fixture.0.join("cargo/rawr/v1/member"), b"shared").unwrap();
        let proof = fixture
            .capture()
            .finish_in(fixture.environment("cargo"))
            .unwrap();
        assert_eq!(proof.0.shared.raw.before.state.bytes, 6);
        assert_eq!(proof.0.project.raw.before.state.bytes, 0);
        assert_eq!(
            proof.0.shared.before_root.path.as_ref(),
            fixture.0.join("cargo/rawr/v1").as_os_str().as_bytes()
        );
        assert!(serde_json::to_vec(&proof).unwrap().len() > 6);
    }

    #[test]
    fn either_durable_tree_change_retains_the_complete_failed_comparison() {
        for relative in ["project/target/rawr/cache/member", "cargo/rawr/v1/member"] {
            let fixture = Fixture::transform();
            let before = fixture.capture();
            fs::write(fixture.0.join(relative), b"changed").unwrap();
            let error = before.finish_in(fixture.environment("cargo")).unwrap_err();
            let observed = error.observed().unwrap();
            assert_eq!(
                observed.project.raw.changes.len() + observed.shared.raw.changes.len(),
                1
            );
            assert!(serde_json::to_vec(observed).unwrap().len() > 7);
        }
    }

    #[test]
    fn unchanged_oracle_rejects_structural_changes_with_identical_byte_totals() {
        use std::os::unix::fs::PermissionsExt as _;

        for mutation in [
            "empty-directory",
            "file-mode",
            "root-mode",
            "file-to-directory",
            "absent-root",
        ] {
            let fixture = Fixture::transform();
            let root = fixture.0.join("project/target/rawr/cache");
            let member = root.join("member");
            if mutation != "absent-root" {
                fs::write(&member, b"same").unwrap();
            }
            let before = fixture.capture();
            match mutation {
                "empty-directory" => fs::create_dir(root.join("empty")).unwrap(),
                "file-mode" => {
                    let mode = fs::metadata(&member).unwrap().mode() & 0o7777;
                    fs::set_permissions(&member, fs::Permissions::from_mode(mode ^ 0o100)).unwrap();
                }
                "root-mode" => {
                    let mode = fs::metadata(&root).unwrap().mode() & 0o7777;
                    fs::set_permissions(&root, fs::Permissions::from_mode(mode ^ 0o1000)).unwrap();
                }
                "file-to-directory" => {
                    fs::remove_file(&member).unwrap();
                    fs::create_dir(&member).unwrap();
                    fs::write(member.join("child"), b"same").unwrap();
                }
                _ => fs::remove_dir(&root).unwrap(),
            }
            let error = before.finish_in(fixture.environment("cargo")).unwrap_err();
            let transition = &error.observed().unwrap().project.raw;
            assert_eq!(transition.before.state.bytes, transition.after.state.bytes);
            assert_ne!(
                transition.before.state.digest,
                transition.after.state.digest
            );
            assert!(!transition.changes.is_empty() || !transition.directory_changes.is_empty());
        }
    }

    #[test]
    fn individually_unchanged_launches_cannot_hide_between_launch_changes() {
        let fixture = Fixture::transform();
        let first = fixture
            .capture()
            .finish_in(fixture.environment("cargo"))
            .unwrap();
        let second = fixture
            .capture()
            .finish_in(fixture.environment("cargo"))
            .unwrap();
        assert!(first.same_state(&second));
        fs::write(fixture.0.join("cargo/rawr/v1/member"), b"between launches").unwrap();
        let third = fixture
            .capture()
            .finish_in(fixture.environment("cargo"))
            .unwrap();
        assert!(!second.same_state(&third));
    }

    #[test]
    fn another_empty_cargo_home_cannot_substitute_for_the_original_store() {
        let fixture = Fixture::transform();
        let error = fixture
            .capture()
            .finish_in(fixture.environment("other"))
            .unwrap_err();
        let observed = error.observed().unwrap();
        assert_ne!(observed.context_before, observed.context_after);
        assert_ne!(observed.shared.before_root, observed.shared.after_root);
    }

    #[test]
    fn cache_aliases_and_absent_shared_warm_store_cannot_look_unchanged() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::transform();
        fs::remove_dir(fixture.0.join("project/target/rawr/cache")).unwrap();
        symlink("missing", fixture.0.join("project/target/rawr/cache")).unwrap();
        assert!(matches!(
            Before::capture_in(
                &fixture.0.join("project"),
                "fixture-target",
                fixture.environment("cargo")
            ),
            Err(Error::Root(_))
        ));
        fs::remove_file(fixture.0.join("project/target/rawr/cache")).unwrap();
        fs::remove_dir(fixture.0.join("cargo/rawr/v1")).unwrap();
        assert!(matches!(
            Before::capture_in(
                &fixture.0.join("project"),
                "fixture-target",
                fixture.environment("cargo")
            ),
            Err(Error::Io { .. })
        ));
    }

    #[test]
    fn failed_post_observation_preserves_the_complete_before_image() {
        let fixture = Fixture::transform();
        let before = fixture.capture();
        fs::remove_dir(fixture.0.join("cargo/rawr/v1")).unwrap();
        let error = before.finish_in(fixture.environment("cargo")).unwrap_err();
        assert!(matches!(error, Error::Incomplete { .. }));
        assert!(error.observed().is_none());
        assert!(serde_json::to_vec(error.before().unwrap()).unwrap().len() > 1);
    }
}
