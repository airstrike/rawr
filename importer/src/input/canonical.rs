//! One observation pass's directory facts.
//!
//! A file path is canonical when its directory is canonical, the directory
//! lists the file's name exactly (case included) and the name is a regular
//! file, not a symlink. `realpath` answers that one component at a time for
//! every file; within one capture or replay pass the directories are shared
//! by many files, so each is canonicalized and listed once. Outside a pass,
//! or when the cached facts cannot confirm a path, callers canonicalize as
//! before.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::rc::Rc;

type Names = Option<Rc<HashSet<OsString>>>;

thread_local! {
    static PASS: RefCell<Option<HashMap<PathBuf, Names>>> = const { RefCell::new(None) };
}

/// Run `pass` with directory facts shared by its observations.
pub(crate) fn pass<T>(pass: impl FnOnce() -> T) -> T {
    let outer = PASS.with(|cell| {
        let mut cell = cell.borrow_mut();
        let outer = cell.is_some();
        if !outer {
            *cell = Some(HashMap::new());
        }
        outer
    });
    struct Clear(bool);
    impl Drop for Clear {
        fn drop(&mut self) {
            if !self.0 {
                PASS.with(|cell| *cell.borrow_mut() = None);
            }
        }
    }
    let _clear = Clear(outer);
    pass()
}

/// Whether this pass's facts confirm `normalized` (absolute, without `.` or
/// `..`) is a canonical regular file; `false` leaves it to `realpath`.
pub(crate) fn regular_file(normalized: &Path) -> bool {
    let (Some(directory), Some(name)) = (normalized.parent(), normalized.file_name()) else {
        return false;
    };
    let names = PASS.with(|cell| {
        let mut cell = cell.borrow_mut();
        let facts = cell.as_mut()?;
        Some(
            facts
                .entry(directory.to_path_buf())
                .or_insert_with(|| list(directory))
                .clone(),
        )
    });
    let Some(Some(names)) = names else {
        return false;
    };
    names.contains(name)
        && std::fs::symlink_metadata(normalized)
            .is_ok_and(|metadata| metadata.file_type().is_file())
}

fn list(directory: &Path) -> Names {
    if std::fs::canonicalize(directory).ok()? != directory {
        return None;
    }
    let names = std::fs::read_dir(directory)
        .ok()?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<HashSet<_>, _>>()
        .ok()?;
    Some(Rc::new(names))
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_pass_confirms_canonical_files_and_leaves_the_rest_to_realpath() {
        let root = std::fs::canonicalize(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../target"),
        )
        .unwrap()
        .join("test-scratch")
        .join(format!("canonical-{}", std::process::id()));
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::write(root.join("real/lib.rs"), b"").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("real/lib.rs"), root.join("real/alias.rs")).unwrap();
        assert!(
            !super::regular_file(&root.join("real/lib.rs")),
            "outside a pass"
        );
        super::pass(|| {
            assert!(super::regular_file(&root.join("real/lib.rs")));
            assert!(
                !super::regular_file(&root.join("real/LIB.rs")),
                "case differs"
            );
            assert!(!super::regular_file(&root.join("real/absent.rs")));
            #[cfg(unix)]
            assert!(
                !super::regular_file(&root.join("link/lib.rs")),
                "a symlinked directory"
            );
            #[cfg(unix)]
            assert!(
                !super::regular_file(&root.join("real/alias.rs")),
                "a symlinked file"
            );
            assert!(!super::regular_file(&root.join("real")), "a directory");
        });
        std::fs::remove_dir_all(root).unwrap();
    }
}
