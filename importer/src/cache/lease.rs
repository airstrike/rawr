//! Cross-process writer exclusion for cache mutation.

use std::fs::OpenOptions;
use std::path::Path;

/// Hold the writer lease of the runner cache at `root`. When another
/// process holds it, say once that this waits, naming that process, and
/// block on the lock. The OS lets go of it when the holder finishes or dies.
pub(crate) fn hold(root: &Path) -> Result<crate::blocking::Exclusive, super::entry::Error> {
    let path = root.join(".writer.lock");
    let io = |operation| {
        let path = path.clone();
        move |source| super::entry::Error::Io {
            operation,
            path,
            source,
        }
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(io("open runner cache writer lease"))?;
    let what = format!("the runner cache at {}", root.display());
    crate::blocking::Exclusive::new(file, &path, &what)
        .map_err(io("acquire runner cache writer lease"))
}
