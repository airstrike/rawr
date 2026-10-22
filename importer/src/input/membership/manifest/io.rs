// Copyright the Rust Project Developers and gitoxide contributors.
// MIT OR Apache-2.0; see load/selection/git/LICENSE-*.
//! The reached Git path-file I/O errors: owned text, then the inner and outer
//! error records, each fallibly retained before the source scope can retire.

use std::{alloc::Layout, fmt, io::ErrorKind, path::Path};

use rawr_toml::document;

use super::{load, session::Session, Error, Step};

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::input::membership) enum Failure {
    #[error("OS error {0}")]
    Native(i32),
    #[error("file name contained an unexpected NUL byte")]
    Nul,
    #[error("{0}")]
    Custom(Owned),
}

/// Both one-record allocations remain real owners. No raw error pointer or
/// infallible std::io::Error constructor hides either allocation from admission.
pub(in crate::input::membership) struct Owned(Vec<Custom>);

struct Message(String);

#[repr(align(4))]
struct Custom {
    kind: ErrorKind,
    error: Vec<Message>,
    error_drop: fn(&mut Vec<Message>),
    outer_drop: fn(&mut Vec<Custom>),
}

impl Failure {
    pub(in crate::input::membership) fn kind(&self) -> ErrorKind {
        match self {
            Self::Native(code) => std::io::Error::from_raw_os_error(*code).kind(),
            Self::Nul => ErrorKind::InvalidInput,
            Self::Custom(value) => value.0[0].kind,
        }
    }

    pub(in crate::input::membership) fn message(&self) -> Option<&str> {
        match self {
            Self::Native(_) => None,
            Self::Nul => Some("file name contained an unexpected NUL byte"),
            Self::Custom(value) => Some(&value.0[0].error[0].0),
        }
    }

    pub(super) fn empty<A: document::Admission>(
        path: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        work(path.as_os_str().len(), 0, session)?;
        Self::formatted(
            ErrorKind::InvalidData,
            format_args!("Refusing to read an empty path from '{}'", path.display()),
            session,
        )
    }

    pub(super) fn parent<A: document::Admission>(
        path: &Path,
        plain: &Path,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        work(path.as_os_str().len(), plain.as_os_str().len(), session)?;
        Self::formatted(
            ErrorKind::Other,
            format_args!(
                "'{path}' has no parent, but '{plain_path}' is relative. It's impossible",
                path = path.display(),
                plain_path = plain.display()
            ),
            session,
        )
    }

    fn oversized<A: document::Admission>(
        path: &Path,
        length: u64,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        work(path.as_os_str().len(), 0, session)?;
        Self::formatted(
            ErrorKind::Other,
            format_args!(
                "Refusing to open files larger than {} bytes, '{}' was {} bytes large",
                65536,
                path.display(),
                length
            ),
            session,
        )
    }

    fn formatted<A: document::Admission>(
        kind: ErrorKind,
        arguments: fmt::Arguments<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let message = load::formatted(arguments, session)?;
        // Pinned From<String> for Box<dyn Error>: move String into StringError,
        // then allocate Custom. Both replacement layouts are derived from the
        // actual safe representation (which owns a Vec instead of a raw ptr).
        let error = retain(Message(message), session)?;
        let custom = Custom {
            kind,
            error,
            error_drop: retire_message,
            outer_drop: retire_custom,
        };
        let outer = retain(custom, session)?;
        Ok(Self::Custom(Owned(outer)))
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if let Some(custom) = self.0.first() {
            let retire = custom.outer_drop;
            retire(&mut self.0);
        }
    }
}

impl Drop for Custom {
    fn drop(&mut self) {
        (self.error_drop)(&mut self.error);
    }
}

fn retire_message(owner: &mut Vec<Message>) {
    drop(std::mem::take(owner));
}
fn retire_custom(owner: &mut Vec<Custom>) {
    drop(std::mem::take(owner));
}

impl fmt::Display for Owned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0[0].error[0].0, formatter)
    }
}
impl fmt::Debug for Owned {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Custom")
            .field("kind", &self.0[0].kind)
            .field("error", &self.0[0].error[0].0)
            .finish()
    }
}
impl PartialEq for Owned {
    fn eq(&self, other: &Self) -> bool {
        self.0[0].kind == other.0[0].kind && self.0[0].error[0].0 == other.0[0].error[0].0
    }
}
impl Eq for Owned {}

fn retain<T, A: document::Admission>(value: T, session: &mut Session<'_, A>) -> Step<Vec<T>> {
    session.items(1)?;
    let layout = Layout::new::<T>();
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut values = Vec::new();
    if let Err(error) = values.try_reserve_exact(1) {
        return session.fail(Error::Allocation(error));
    }
    values.push(value);
    Ok(values)
}

fn work<A: document::Admission>(
    left: usize,
    right: usize,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.string(left)?;
    session.string(right)?;
    // Both pure Path::display passes, including worst-case replacement chars,
    // fixed literals and the two bounded integer displays, precede retention.
    let Some(work) = left
        .checked_add(right)
        .and_then(|n| n.checked_mul(6))
        .and_then(|n| n.checked_add(256))
    else {
        return session.fail(Error::Overflow);
    };
    session.items(work)
}

/// The closed acquisition has a Budget rather than a manifest Session. This
/// local latch forwards its first error immediately; it creates no new account.
pub(in crate::input::membership) fn oversized(
    path: &Path,
    length: u64,
    budget: &mut crate::input::membership::Budget<'_>,
) -> crate::input::membership::Result<Failure> {
    use crate::input::membership;
    let mut session = Session::new(budget);
    let result = Failure::oversized(path, length, &mut session);
    session.finish(result).map_err(|error| match error {
        Error::Admission(error) | Error::Host(error) => error,
        Error::Allocation(error) => membership::Error::Allocation(error),
        Error::Overflow => membership::Error::DocumentOverflow,
        _ => membership::Error::Encoding("Git path error construction failed"),
    })
}

#[cfg(test)]
mod tests;
