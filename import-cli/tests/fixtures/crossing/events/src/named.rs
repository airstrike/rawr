//! Functions generic over what converts into or lends text, a path or
//! bytes, as crates take them: `S: Into<String>`, `impl AsRef<str>`,
//! `P: AsRef<Path>`, `impl Into<PathBuf>`, `B: AsRef<[u8]>` and `T:
//! ToString`. What an argument may be follows from std's impls of the
//! bound for its type, not from how the parameter is spelled.

use std::path::{Path, PathBuf};

/// `name` as a greeting, through a named type parameter.
pub fn named<S: Into<String>>(name: S) -> String {
    let name: String = name.into();
    format!("named {name}")
}

/// `name` as a greeting, through `impl Trait`.
pub fn spelled(name: impl Into<String>) -> String {
    let name: String = name.into();
    format!("spelled {name}")
}

/// How long `text` is, through a named type parameter.
pub fn measured<S: AsRef<str>>(text: S) -> usize {
    text.as_ref().len()
}

/// `path`'s file name and parent, borrowed through `AsRef<Path>`.
pub fn opened<P: AsRef<Path>>(path: P) -> String {
    let path = path.as_ref();
    format!(
        "opened {} in {}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("?"),
        path.parent()
            .map(|parent| parent.display().to_string())
            .unwrap_or_default()
    )
}

/// `path` taken whole, through `Into<PathBuf>`.
pub fn owned(path: impl Into<PathBuf>) -> String {
    let mut path: PathBuf = path.into();
    path.push("owned");
    path.display().to_string()
}

/// The sum of `bytes`, borrowed through `AsRef<[u8]>`.
pub fn summed<B: AsRef<[u8]>>(bytes: B) -> u32 {
    bytes.as_ref().iter().map(|byte| u32::from(*byte)).sum()
}

/// `value` as text, through `ToString`.
pub fn shown<T: ToString>(value: T) -> String {
    format!("[{}]", value.to_string())
}
