//! Parent observation of the exact pinned driver's explicit sysroot selection.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::extract::Allocation;
use crate::{Error, Result};

const COMPILER: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";

/// Current selector facts only; directory membership and content are separate.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Search {
    pub(super) rustc: PathBuf,
    pub(super) sysroot: PathBuf,
    pub(super) library: PathBuf,
    pub(super) lib64: bool,
}

impl Search {
    pub(super) fn revalidate(&self, allocation: &mut Allocation) -> Result<()> {
        let current = Self::observe(
            &self.rustc,
            COMPILER,
            crate::cargo::toolchain::HOST,
            crate::cargo::toolchain::HOST,
            allocation,
        )?;
        if &current != self {
            return Err(invalid("compiler search selector changed"));
        }
        Ok(())
    }

    pub(super) fn observe(
        rustc: &Path,
        compiler: &str,
        host: &str,
        target: &str,
        allocation: &mut Allocation,
    ) -> Result<Self> {
        if compiler != COMPILER || host != crate::cargo::toolchain::HOST || target != host {
            return Err(invalid("compiler search pin differs"));
        }
        if !rustc.is_absolute()
            || rustc
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(invalid("compiler path is not absolute and normalized"));
        }
        let rustc = canonical(rustc, allocation)?;
        let metadata =
            fs::symlink_metadata(&rustc).map_err(|source| io(&rustc, source, allocation))?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(invalid("canonical compiler is not a regular file"));
        }
        // This is the production driver.rs derivation, not rustc's ambient
        // default sysroot or the parent of the Rawr driver executable.
        let parent = rustc
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| invalid("compiler has no sysroot grandparent"))?;
        let sysroot = canonical(parent, allocation)?;
        let metadata =
            fs::symlink_metadata(&sysroot).map_err(|source| io(&sysroot, source, allocation))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(invalid("canonical sysroot is not a directory"));
        }
        let selector = join(&sysroot, "lib64/rustlib", allocation)?;
        // The pinned compiler uses exists(), not is_dir(). Error cases refuse
        // conservatively rather than becoming evidence of selector absence.
        let lib64 = match fs::metadata(&selector) {
            Ok(_) => true,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(io(&selector, error, allocation)),
        };
        let base = join(
            &sysroot,
            if lib64 {
                "lib64/rustlib"
            } else {
                "lib/rustlib"
            },
            allocation,
        )?;
        let target = join(&base, target, allocation)?;
        let library = join(&target, "lib", allocation)?;
        Ok(Self {
            rustc,
            sysroot,
            library,
            lib64,
        })
    }
}

fn canonical(path: &Path, allocation: &mut Allocation) -> Result<PathBuf> {
    allocation.path_capacity(1024)?;
    let value = fs::canonicalize(path).map_err(|source| io(path, source, allocation))?;
    allocation.validate_path(&value, 1024)?;
    Ok(value)
}

fn join(parent: &Path, suffix: &str, allocation: &mut Allocation) -> Result<PathBuf> {
    let size = parent
        .as_os_str()
        .len()
        .checked_add(1)
        .and_then(|size| size.checked_add(suffix.len()))
        .ok_or_else(|| graph_error(crate::node::Error::Allocation))?;
    allocation.path_capacity(size)?;
    let mut path = PathBuf::new();
    path.try_reserve_exact(size)
        .map_err(|_| graph_error(crate::node::Error::Allocation))?;
    path.push(parent);
    path.push(suffix);
    Ok(path)
}

fn graph_error(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "compiler search selection",
        source: Box::new(source),
    }
}
fn invalid(reason: &'static str) -> Error {
    Error::Extraction {
        code: "compiler-search-selection",
        reason: reason.into(),
    }
}
fn io(path: &Path, source: std::io::Error, allocation: &mut Allocation) -> Error {
    match allocation.clone_path(path) {
        Ok(path) => Error::Io {
            operation: "observe compiler search selection",
            path,
            source,
        },
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_selector_preserves_existence_and_original_accounts() {
        let root = std::env::temp_dir().join(format!("rawr-parent-search-{}", std::process::id()));
        fs::create_dir_all(root.join("bin")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let rustc = root.join("bin/rustc");
        fs::write(&rustc, b"fixture").unwrap();
        let observe = || {
            Search::observe(
                &rustc,
                COMPILER,
                crate::cargo::toolchain::HOST,
                crate::cargo::toolchain::HOST,
                &mut Allocation::with_limits(100, 65536),
            )
            .unwrap()
        };
        let absent = observe();
        assert!(!absent.lib64);
        assert_eq!(
            absent.library,
            root.join("lib/rustlib")
                .join(crate::cargo::toolchain::HOST)
                .join("lib")
        );
        fs::create_dir(root.join("lib64")).unwrap();
        fs::write(
            root.join("lib64/rustlib"),
            b"regular file still selects lib64",
        )
        .unwrap();
        let present = observe();
        assert!(present.lib64);
        assert_ne!(absent, present);
        let spent = Search::observe(
            &rustc,
            COMPILER,
            crate::cargo::toolchain::HOST,
            crate::cargo::toolchain::HOST,
            &mut Allocation::with_limits(100, 65536),
        )
        .unwrap();
        assert_eq!(spent, present);
        // A zero-limit account observes the same selector.
        let mut recorded = Allocation::with_limits(0, 0);
        assert_eq!(
            Search::observe(
                &rustc,
                COMPILER,
                crate::cargo::toolchain::HOST,
                crate::cargo::toolchain::HOST,
                &mut recorded,
            )
            .unwrap(),
            present
        );
        assert!(!recorded.is_refused());
        let (items, bytes) = recorded.usage();
        assert!(items > 0 && bytes > 0);
        // Terminal only through a genuine cause: a latched account owns no path.
        assert!(matches!(
            Search::observe(
                &rustc,
                COMPILER,
                crate::cargo::toolchain::HOST,
                crate::cargo::toolchain::HOST,
                &mut Allocation::refused(),
            ),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
        // Error ownership charges the original account: a missing compiler
        // reports its I/O failure while the one-item account records the
        // owned diagnostic path at exactly target + 1.
        let missing = root.join("bin/missing");
        let mut short = Allocation::with_limits(1, 65536);
        assert!(matches!(
            Search::observe(
                &missing,
                COMPILER,
                crate::cargo::toolchain::HOST,
                crate::cargo::toolchain::HOST,
                &mut short,
            ),
            Err(Error::Io {
                operation: "observe compiler search selection",
                ..
            })
        ));
        assert!(!short.is_refused());
        // A latched account replaces the diagnostic with its own refusal.
        assert!(matches!(
            Search::observe(
                &missing,
                COMPILER,
                crate::cargo::toolchain::HOST,
                crate::cargo::toolchain::HOST,
                &mut Allocation::refused(),
            ),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
