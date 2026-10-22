//! Symlink-safe inventory proof for a complete baseline tree.

use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::Error;

#[derive(Debug)]
pub(super) struct Inventory {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl Inventory {
    pub(super) fn read(root: &Path) -> Result<Self, Error> {
        let metadata = match fs::symlink_metadata(root) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self {
                    files: BTreeMap::new(),
                });
            }
            Err(source) => return Err(Error::io(root, source)),
        };
        directory(root, &metadata)?;
        let mut files = BTreeMap::new();
        walk(root, root, &mut files)?;
        Ok(Self { files })
    }

    pub(super) fn paths(&self) -> impl Iterator<Item = &Path> {
        self.files.keys().map(PathBuf::as_path)
    }

    pub(super) fn bytes(&self, path: &Path) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }
}

fn walk(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(), Error> {
    let entries = fs::read_dir(directory).map_err(|source| Error::io(directory, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::io(directory, source))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::io(&path, source))?;
        if metadata.file_type().is_symlink() {
            return Err(Error::UnsafePath { path });
        }
        if metadata.is_dir() {
            walk(root, &path, files)?;
        } else if metadata.is_file() {
            let mut file = fs::File::open(&path).map_err(|source| Error::io(&path, source))?;
            let opened = file.metadata().map_err(|source| Error::io(&path, source))?;
            let retained =
                fs::symlink_metadata(&path).map_err(|source| Error::io(&path, source))?;
            if retained.file_type().is_symlink()
                || !retained.is_file()
                || !same_file(&opened, &retained)
            {
                return Err(Error::UnsafePath { path });
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)
                .map_err(|source| Error::io(&path, source))?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::UnsafePath { path: path.clone() })?
                .to_path_buf();
            files.insert(relative, bytes);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_file(opened: &fs::Metadata, retained: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    opened.dev() == retained.dev() && opened.ino() == retained.ino()
}

#[cfg(not(unix))]
fn same_file(opened: &fs::Metadata, retained: &fs::Metadata) -> bool {
    opened.len() == retained.len()
        && opened.created().ok() == retained.created().ok()
        && opened.modified().ok() == retained.modified().ok()
}

fn directory(path: &Path, metadata: &fs::Metadata) -> Result<(), Error> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Error::UnsafePath {
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    #[cfg(unix)]
    #[test]
    fn manifest_symlink_is_rejected_before_external_bytes_are_consumed() {
        use std::os::unix::fs::symlink;

        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rawr-snapshot-inventory-{}-{id}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        let outside = root.with_extension("external");
        let bytes = b"not even UTF-8: \xff";
        fs::write(&outside, bytes).unwrap();
        let manifest = root.join("manifest.rawr-snapshot");
        symlink(&outside, &manifest).unwrap();

        let error = super::Inventory::read(&root).unwrap_err();
        let super::Error::UnsafePath { path } = error else {
            panic!("expected symlink rejection, got {error}");
        };
        assert_eq!(path, manifest);
        assert_eq!(fs::read(&outside).unwrap(), bytes);

        fs::remove_file(manifest).unwrap();
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn nested_png_symlink_is_rejected_before_external_bytes_are_consumed() {
        use std::os::unix::fs::symlink;

        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "rawr-snapshot-png-inventory-{}-{id}",
            std::process::id()
        ));
        let directory = root.join("package/view/default");
        fs::create_dir_all(&directory).unwrap();
        let outside = root.with_extension("external-png");
        let bytes = b"not a PNG and must never be decoded";
        fs::write(&outside, bytes).unwrap();
        let png = directory.join("native.png");
        symlink(&outside, &png).unwrap();

        let error = super::Inventory::read(&root).unwrap_err();
        let super::Error::UnsafePath { path } = error else {
            panic!("expected symlink rejection, got {error}");
        };
        assert_eq!(path, png);
        assert_eq!(fs::read(&outside).unwrap(), bytes);

        fs::remove_file(png).unwrap();
        fs::remove_file(outside).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
