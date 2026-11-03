//! A crate whose items name std's `PathBuf`, `&Path` and `io::Error`, with
//! an error enum that converts from `io::Error` as thiserror's `#[from]`
//! writes it.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// A file the crate lists, holding its path.
#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub size: u64,
}

impl Entry {
    pub fn new(path: PathBuf, size: u64) -> Self {
        Self { path, size }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// The directory every entry is under.
pub fn root() -> PathBuf {
    PathBuf::from("data")
}

/// How many components `path` has.
pub fn depth(path: &Path) -> usize {
    path.components().count()
}

/// An entry for `path`.
pub fn entry(path: &Path, size: u64) -> Entry {
    Entry::new(path.to_path_buf(), size)
}

/// What a read fails with.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Empty(PathBuf),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "io: {error}"),
            Error::Empty(path) => write!(f, "{} is empty", path.display()),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Error::Io(error)
    }
}

/// The error's kind.
pub fn kind(error: &io::Error) -> io::ErrorKind {
    error.kind()
}

/// An `io::Error` the crate makes.
pub fn missing(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} is missing", path.display()),
    )
}

/// The path of `name` under `root`, which an empty name has none of.
pub fn under(root: &Path, name: &str) -> Result<PathBuf, Error> {
    if name.is_empty() {
        return Err(Error::Empty(root.to_path_buf()));
    }
    Ok(root.join(name))
}

/// The text of the file at `path`, read by the crate.
pub fn read(path: &Path) -> Result<String, Error> {
    let text = std::fs::read_to_string(path)?;
    if text.is_empty() {
        return Err(Error::Empty(path.to_path_buf()));
    }
    Ok(text)
}

/// Appends `name` to `path`, in place.
pub fn extend(path: &mut PathBuf, name: &str) {
    path.push(name);
}

/// Makes room in `path` for `extra` more bytes.
pub fn reserve(path: &mut PathBuf, extra: usize) {
    path.reserve(extra);
}

/// Whether `path` has room for 100 bytes: what `reserve` made, which a
/// copy of it has none of.
pub fn roomy(path: &mut PathBuf) -> bool {
    path.capacity() >= 100
}

/// A failed read, which holds its `io::Error`.
#[derive(Debug)]
pub struct Failure {
    error: io::Error,
    pub path: PathBuf,
}

impl Failure {
    pub fn new(path: PathBuf) -> Self {
        let error = missing(&path);
        Self { error, path }
    }

    /// The error, borrowed from the failure.
    pub fn error(&self) -> &io::Error {
        &self.error
    }

    /// Appends the failure's path to `out`, in place.
    pub fn append_to(&self, out: &mut PathBuf) {
        out.push(&self.path);
    }
}
