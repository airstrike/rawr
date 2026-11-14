//! The stable copy of a host's sources that a shared build compiles.
//!
//! Every host whose build agrees on the base key compiles in one cargo
//! build directory (`build`). Its sources sit at one path there, and each
//! build first makes that copy hold exactly the host's files, writing only
//! the ones whose bytes differ and removing the ones the host lacks. A file
//! left alone keeps its modification time, so cargo's own fingerprints find
//! dirty exactly the units whose files changed (#1983).

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Make `copy` hold exactly the files of the host at `host`. Names that start
/// with a dot are the host's own bookkeeping (its `.digest` stamp and lease
/// files) and are never copied: they change with every host while no unit
/// reads them.
pub(super) fn mirror(host: &Path, copy: &Path) -> io::Result<()> {
    let mut files = Vec::new();
    let mut directories = BTreeSet::new();
    walk(host, Path::new(""), &mut files, &mut directories)?;
    let wanted = files.iter().cloned().collect::<BTreeSet<_>>();
    remove_extra(copy, Path::new(""), &wanted, &directories)?;
    for relative in &files {
        let bytes = fs::read(host.join(relative))?;
        let path = copy.join(relative);
        if fs::read(&path).is_ok_and(|kept| kept == bytes) {
            continue;
        }
        if path.is_dir() {
            fs::remove_dir_all(&path)?;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &bytes)?;
    }
    Ok(())
}

/// Every file below `root/relative`, and every directory, relative to
/// `root`, leaving out names that start with a dot.
fn walk(
    root: &Path,
    relative: &Path,
    files: &mut Vec<PathBuf>,
    directories: &mut BTreeSet<PathBuf>,
) -> io::Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            directories.insert(path.clone());
            walk(root, &path, files, directories)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Remove from `copy/relative` each file `wanted` lacks and each directory
/// `directories` lacks. A copy not written yet has nothing to remove.
fn remove_extra(
    copy: &Path,
    relative: &Path,
    wanted: &BTreeSet<PathBuf>,
    directories: &BTreeSet<PathBuf>,
) -> io::Result<()> {
    let entries = match fs::read_dir(copy.join(relative)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = relative.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            if directories.contains(&path) {
                remove_extra(copy, &path, wanted, directories)?;
            } else {
                fs::remove_dir_all(entry.path())?;
            }
        } else if !wanted.contains(&path) {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let path = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-runtime-source-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn a_mirror_rewrites_only_the_files_that_changed() {
        let root = scratch("mirror");
        let first = root.join("first");
        let second = root.join("second");
        let copy = root.join("copy");
        for (host, resolver) in [(&first, "one"), (&second, "two")] {
            fs::create_dir_all(host.join("src/resolve")).unwrap();
            fs::create_dir_all(host.join("watch/src")).unwrap();
            fs::write(host.join("src/resolve/coerce.rs"), resolver).unwrap();
            fs::write(host.join("src/lib.rs"), "lib").unwrap();
            fs::write(host.join(".digest"), resolver).unwrap();
        }
        fs::write(first.join("watch/src/old.rs"), "old").unwrap();
        fs::create_dir_all(first.join("gone")).unwrap();
        fs::write(first.join("gone/file.rs"), "gone").unwrap();
        super::mirror(&first, &copy).unwrap();
        assert_eq!(
            fs::read(copy.join("src/resolve/coerce.rs")).unwrap(),
            b"one"
        );
        assert!(
            !copy.join(".digest").exists(),
            "the host's stamp changes with every host and no unit reads it"
        );
        let modified = |path: &str| fs::metadata(copy.join(path)).unwrap().modified().unwrap();
        let untouched = modified("src/lib.rs");
        super::mirror(&second, &copy).unwrap();
        assert_eq!(
            fs::read(copy.join("src/resolve/coerce.rs")).unwrap(),
            b"two"
        );
        assert_eq!(
            modified("src/lib.rs"),
            untouched,
            "a file whose bytes agree keeps its time, so cargo finds it fresh"
        );
        assert!(!copy.join("watch/src/old.rs").exists());
        assert!(!copy.join("gone").exists());
        assert!(copy.join("watch/src").is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
