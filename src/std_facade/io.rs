//! std's `io::Error` and `io::ErrorKind` for Rawr code. An error is Rust's
//! own `std::io::Error`, which a crate's code takes and borrows as it is.
//! `stdin()` reads the process's stdin through Rust's own, and `stdout()`
//! flushes Rust's own. Nothing here reads or writes a file.

use std::io::ErrorKind;
use std::sync::{Arc, Mutex};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// What rawr runs for a stream's value, `Stdin` or `Stdout`, which holds
/// nothing of its own.
const STREAM: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::Copy,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The types the module holds.
pub(super) const HELD: [Held; 4] = [
    Held::Value(held::Value {
        std: "std::io::Error",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder.type_::<Error>(path, &[], capabilities).map(drop)
        },
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::io::ErrorKind",
        lent: &[],
        also: &[],
        register: |builder, path, capabilities| {
            builder
                .value_type::<ErrorKind>(path, &[], capabilities)
                .map(drop)
        },
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Hash,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::io::Stdin",
        lent: &[],
        also: &["std::io::StdinLock"],
        register: |builder, path, capabilities| {
            builder
                .default_value_type::<Stdin>(path, &[], capabilities)
                .map(drop)
        },
        runs: STREAM,
    }),
    Held::Value(held::Value {
        std: "std::io::Stdout",
        lent: &[],
        also: &["std::io::StdoutLock"],
        register: |builder, path, capabilities| {
            builder
                .default_value_type::<Stdout>(path, &[], capabilities)
                .map(drop)
        },
        runs: STREAM,
    }),
];

/// std's `Stdin`, and its `StdinLock`: the process's stdin, read through
/// Rust's own buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stdin;

/// std's `Stdout`, and its `StdoutLock`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stdout;

/// Rust's `std::io::Error`, which is not `Clone`. A Rawr value is a handle
/// to the error's one place, so copies of the value share it. Native code
/// borrows the error itself, and takes it out of its place when the program
/// moves it, which leaves no handle that is read again.
#[derive(Clone)]
pub(crate) struct Error(Arc<Mutex<Option<Arc<std::io::Error>>>>);

impl Error {
    fn new(error: std::io::Error) -> Self {
        Self(Arc::new(Mutex::new(Some(Arc::new(error)))))
    }

    /// The error, lent: the program never reads a value it moved.
    fn get(&self) -> Arc<std::io::Error> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .unwrap_or_else(|| panic!("rawr: a moved `std::io::Error` was read"))
    }
}

#[cfg(feature = "sync")]
impl crate::crossing::Std for std::io::Error {
    type Lent = Arc<Self>;

    fn give(self) -> Dynamic {
        made(self)
    }

    /// The error, moved out of its place. No borrow of it outlives the
    /// move, so it is the only one.
    fn take(value: Dynamic) -> Option<Self> {
        let error = value
            .try_cast::<Error>()?
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()?;
        Some(
            Arc::try_unwrap(error).unwrap_or_else(|_| {
                panic!("rawr: a `std::io::Error` was moved while it was borrowed")
            }),
        )
    }

    fn lend(value: Dynamic) -> Option<Arc<Self>> {
        value.try_cast::<Error>().map(|error| error.get())
    }
}

impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.get(), formatter)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.get(), formatter)
    }
}

/// Register `Debug` and `Display` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Error>();
    crate::host::debug::register::<ErrorKind>();
    crate::host::display::register::<Error>();
    crate::host::display::register::<ErrorKind>();
}

/// What one read of stdin into the `&mut String` `buffer` gives: its
/// `io::Result`, then the buffer with what it read appended, which goes
/// back to the place it was borrowed from.
fn appended(
    buffer: &Dynamic,
    read: impl FnOnce(&mut String) -> std::io::Result<usize>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let mut text = read_value::<ImmutableString>(buffer, "String")?.to_string();
    let result = match read(&mut text) {
        Ok(count) => crate::result::ok(Dynamic::from(count as crate::INT)),
        Err(error) => crate::result::err(made(error)),
    };
    Ok(Dynamic::from(vec![
        result,
        Dynamic::from(ImmutableString::from(text)),
    ]))
}

/// Bind `stdin()`, `stdout()` and their methods.
fn bind_streams(binds: &mut impl super::Bind) {
    use std::io::{BufRead as _, Read as _, Write as _};

    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("io::stdin", |_| Ok(Dynamic::from(Stdin)));
    bind("io::stdout", |_| Ok(Dynamic::from(Stdout)));
    bind("io::Stdin::lock", |_| Ok(Dynamic::from(Stdin)));
    bind("io::Stdout::lock", |_| Ok(Dynamic::from(Stdout)));
    bind("io::Stdin::read_line", |args| {
        appended(args[1], |text| std::io::stdin().lock().read_line(text))
    });
    bind("io::Stdin::read_to_string", |args| {
        appended(args[1], |text| std::io::stdin().lock().read_to_string(text))
    });
    bind("io::Stdin::lines", |_| {
        let lines = std::io::stdin()
            .lock()
            .lines()
            .map(|line| match line {
                Ok(line) => crate::result::ok(Dynamic::from(ImmutableString::from(line))),
                Err(error) => crate::result::err(made(error)),
            })
            .collect::<crate::Array>();
        Ok(Dynamic::from(lines))
    });
    bind("io::Stdout::flush", |_| {
        Ok(match std::io::stdout().flush() {
            Ok(()) => crate::result::ok(Dynamic::UNIT),
            Err(error) => crate::result::err(made(error)),
        })
    });
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn read_value<T: Clone + 'static>(
    value: &Dynamic,
    expected: &str,
) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            error(format!(
                "expected `{expected}`, found `{}`",
                value.type_name()
            ))
        })
}

/// A Rawr `std::io::Error` holding `error`.
pub(crate) fn made(error: std::io::Error) -> Dynamic {
    Dynamic::from(Error::new(error))
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    macro_rules! kinds {
        ($($name:ident)*) => {
            $(bind(
                concat!("io::ErrorKind::", stringify!($name)),
                |_| Ok(Dynamic::from(ErrorKind::$name)),
            );)*
        };
    }
    kinds!(
        NotFound PermissionDenied ConnectionRefused ConnectionReset ConnectionAborted
        NotConnected AddrInUse AddrNotAvailable BrokenPipe AlreadyExists WouldBlock
        InvalidInput InvalidData TimedOut WriteZero Interrupted Unsupported UnexpectedEof
        OutOfMemory Other FileTooLarge
    );
    bind("io::Error::new", |args| {
        let kind = read_value::<ErrorKind>(args[0], "ErrorKind")?;
        let text = read_value::<ImmutableString>(args[1], "String")?;
        Ok(made(std::io::Error::new(kind, text.to_string())))
    });
    bind("io::Error::other", |args| {
        let text = read_value::<ImmutableString>(args[0], "String")?;
        Ok(made(std::io::Error::other(text.to_string())))
    });
    bind("io::Error::from", |args| {
        // `From<T> for T`: the error itself.
        if args[0].is::<Error>() {
            return Ok(args[0].take());
        }
        Ok(made(std::io::Error::from(read_value::<ErrorKind>(
            args[0],
            "ErrorKind",
        )?)))
    });
    bind("io::Error::kind", |args| {
        Ok(Dynamic::from(
            read_value::<Error>(args[0], "io::Error")?.get().kind(),
        ))
    });
    bind("io::Error::to_string", |args| {
        Ok(Dynamic::from(ImmutableString::from(
            read_value::<Error>(args[0], "io::Error")?.to_string(),
        )))
    });
    bind("io::ErrorKind::to_string", |args| {
        Ok(Dynamic::from(ImmutableString::from(
            read_value::<ErrorKind>(args[0], "ErrorKind")?.to_string(),
        )))
    });
    bind_streams(binds);
}
