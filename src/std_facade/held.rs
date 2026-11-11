//! How rawr holds each std type the facade represents: the binding for
//! std's types (D711.1). It states the Rust value and what rawr runs for
//! it; std's table states the rest.

use crate::capability;
use crate::host::{self, facade};

/// Declares a held type's Rust value at its facade path, with the
/// capabilities std gives it that rawr runs.
pub(crate) type Register =
    fn(&mut facade::Builder, &str, &[capability::Kind]) -> Result<(), host::Error>;

/// A std type rawr represents.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "only `declare` reads it, to write `declared`")
)]
pub(crate) enum Held {
    /// A Rust value of its own, as `PathBuf` is std's own `PathBuf`.
    Value(Value),
    /// rawr's string, holding the text the type's `Display` writes:
    /// `std::path::Display`, by its public path.
    Text(&'static str),
}

/// A std type held as a Rust value of its own.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "only `declare` reads it, to write `declared`")
)]
pub(crate) struct Value {
    /// Its public path, which is its facade path under `std`:
    /// `std::path::PathBuf` is `path::PathBuf`.
    pub std: &'static str,
    /// The unsized types a borrow of it stands for: `std::path::Path` of
    /// `PathBuf`, whose methods are its own through `Deref`.
    pub lent: &'static [&'static str],
    /// The sized types it also stands for: `std::io::StdinLock` of
    /// `Stdin`, which reads the same stdin.
    pub also: &'static [&'static str],
    pub register: Register,
    /// Whether a crate takes, gives and borrows the value as std's own
    /// (`crossing::Std`), as it does a `PathBuf` or an `io::Error`.
    pub crosses: bool,
    /// The capabilities rawr runs for the value. Each is declared when std
    /// implements its trait for the type too.
    pub runs: &'static [capability::Kind],
    /// std's constants of the type, by their public paths:
    /// `std::time::UNIX_EPOCH` of `SystemTime`. std's table doesn't record
    /// a constant's type, so the binding states it.
    pub constants: &'static [&'static str],
}

impl Value {
    /// The public paths of every type it stands for, its own first.
    pub(crate) fn forms(&self) -> impl Iterator<Item = &'static str> {
        std::iter::once(self.std)
            .chain(self.lent.iter().copied())
            .chain(self.also.iter().copied())
    }

    /// The facade path of the type at the public path `std`.
    pub(crate) fn facade(std: &str) -> &str {
        std.strip_prefix("std::").unwrap_or(std)
    }
}
