//! std's `io::Error` and `io::ErrorKind` for Rawr code
//! (`resolve::std_reexport` sends `std::io` here). An error is Rust's own
//! `std::io::Error`, which a crate's code takes and borrows as it is. `stdin()` reads
//! the process's stdin through Rust's own, and `stdout()` flushes Rust's
//! own. Nothing here reads or writes a file.

use std::io::ErrorKind;
use std::sync::{Arc, Mutex};

use crate::host::{self, facade, item, signature, type_ref};
use crate::{capability, receiver, Dynamic, EvalAltResult, ImmutableString, Position};

const ERROR: &str = "io::Error";
const KIND: &str = "io::ErrorKind";
const STDIN: &str = "io::Stdin";
const STDOUT: &str = "io::Stdout";

/// The methods that append what they read to a `&mut String`, as Rust's
/// readers do. Each callback gives `(result, text)`, and the checker
/// appends the text to the argument's place (`finish_host_method`).
pub(crate) const APPENDING: [&str; 2] = [
    "rawr_std::io::Stdin::read_line",
    "rawr_std::io::Stdin::read_to_string",
];

/// std's `Stdin`, and its `StdinLock`: the process's stdin, read through
/// Rust's own buffer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stdin;

/// std's `Stdout`, and its `StdoutLock`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Stdout;

/// std's `io::Read`, `io::BufRead` and `io::Write`, which a program
/// imports for the methods above.
struct Read;
struct BufRead;
struct Write;

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

/// The `ErrorKind` variants Rawr names, each as Rust spells it.
const KINDS: [&str; 21] = [
    "NotFound",
    "PermissionDenied",
    "ConnectionRefused",
    "ConnectionReset",
    "ConnectionAborted",
    "NotConnected",
    "AddrInUse",
    "AddrNotAvailable",
    "BrokenPipe",
    "AlreadyExists",
    "WouldBlock",
    "InvalidInput",
    "InvalidData",
    "TimedOut",
    "WriteZero",
    "Interrupted",
    "Unsupported",
    "UnexpectedEof",
    "OutOfMemory",
    "Other",
    "FileTooLarge",
];

/// Register `Debug` and `Display` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Error>();
    crate::host::debug::register::<ErrorKind>();
    crate::host::display::register::<Error>();
    crate::host::display::register::<ErrorKind>();
}

/// Declare the types and their items in the std facade, under `io`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use capability::Kind::{Clone, Copy, Debug, Eq, Hash, PartialEq, Send, Static};
    use type_ref::{Builtin, Type};

    builder.module("io")?;
    builder.type_::<Error>(ERROR, &[], &[Debug, Send, Static])?;
    builder.value_type::<ErrorKind>(
        KIND,
        &[],
        &[Debug, Clone, Copy, PartialEq, Eq, Hash, Send, Static],
    )?;
    // std's `io::Result<T>`, which is `Result<T, io::Error>`.
    builder.type_alias(
        "io::Result",
        Box::new(["T".into()]),
        Box::default(),
        Type::result(Type::parameter("T")?, Type::registered(ERROR)?),
    )?;
    for name in KINDS {
        builder.item(item::Definition::associated_value(
            format!("{KIND}::{name}"),
            Type::registered(KIND)?,
        )?)?;
    }

    let string = || Type::builtin(Builtin::String);
    let error = || Type::registered(ERROR);
    let kind = || Type::registered(KIND);
    let signature = |inputs: Vec<Type>, output: Type| {
        let mut signature = signature::Builder::new();
        for input in inputs {
            signature.parameter(input);
        }
        signature.output(output);
        signature.finish()
    };
    let mut function = |path: String, inputs: Vec<Type>, output: Type| {
        builder.item(item::Definition::associated_function(
            path,
            signature(inputs, output),
        )?)?;
        Ok::<_, host::Error>(())
    };
    // `impl Into<Box<dyn Error + Send + Sync>>`: a `&str` or a `String`.
    let text = || Type::reference(false, string());
    for message in [text(), string()] {
        function(
            format!("{ERROR}::new"),
            vec![kind()?, message.clone()],
            error()?,
        )?;
        function(format!("{ERROR}::other"), vec![message], error()?)?;
    }
    function(format!("{ERROR}::from"), vec![kind()?], error()?)?;
    let mut method = |path: String, inputs: Vec<Type>, output: Type| {
        builder.item(item::Definition::method(
            path,
            receiver::Mode::Shared,
            signature(inputs, output),
        )?)?;
        Ok::<_, host::Error>(())
    };
    method(format!("{ERROR}::kind"), vec![], kind()?)?;
    method(format!("{ERROR}::to_string"), vec![], string())?;
    method(format!("{KIND}::to_string"), vec![], string())?;
    declare_streams(builder)
}

/// `stdin()`, `stdout()` and their methods, and the traits that name them.
fn declare_streams(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use capability::Kind::{Clone, Copy, Debug, Default, Eq, PartialEq, Send, Static};
    use type_ref::{Builtin, Type};

    const CAPABILITIES: &[capability::Kind] =
        &[Default, Debug, Clone, Copy, PartialEq, Eq, Send, Static];
    builder.default_value_type::<Stdin>(STDIN, &[], CAPABILITIES)?;
    builder.default_value_type::<Stdout>(STDOUT, &[], CAPABILITIES)?;
    builder.alias("io::StdinLock", STDIN)?;
    builder.alias("io::StdoutLock", STDOUT)?;
    builder.trait_::<Read>("io::Read", &[])?;
    builder.trait_::<BufRead>("io::BufRead", &[])?;
    builder.trait_::<Write>("io::Write", &[])?;

    let string = || Type::builtin(Builtin::String);
    let result = |ok: Type| Ok::<_, host::Error>(Type::result(ok, Type::registered(ERROR)?));
    let signature = |output: Type| {
        let mut signature = signature::Builder::new();
        signature.output(output);
        signature.finish()
    };
    builder.item(item::Definition::function(
        "io::stdin",
        signature(Type::registered(STDIN)?),
    )?)?;
    builder.item(item::Definition::function(
        "io::stdout",
        signature(Type::registered(STDOUT)?),
    )?)?;
    let mut method = |path: String, output: Type| {
        builder.item(item::Definition::method(
            path,
            receiver::Mode::Shared,
            signature(output),
        )?)?;
        Ok::<_, host::Error>(())
    };
    method(format!("{STDIN}::lock"), Type::registered(STDIN)?)?;
    // `read_line(&mut buf)` and `read_to_string(&mut buf)`: see
    // [`APPENDING`].
    let appended = Type::tuple([result(Type::builtin(Builtin::Usize))?, string()])?;
    method(format!("{STDIN}::read_line"), appended.clone())?;
    method(format!("{STDIN}::read_to_string"), appended)?;
    // Every line up to the end of input, read when `lines()` is called.
    method(format!("{STDIN}::lines"), Type::iterator(result(string())?))?;
    method(format!("{STDOUT}::lock"), Type::registered(STDOUT)?)?;
    method(format!("{STDOUT}::flush"), result(Type::unit())?)?;
    Ok(())
}

/// What one read of stdin gives a Rawr program: `(Ok(bytes), text)` or
/// `(Err(error), "")`.
fn appended(read: impl FnOnce(&mut String) -> std::io::Result<usize>) -> Dynamic {
    let mut text = String::new();
    let result = match read(&mut text) {
        Ok(count) => crate::result::ok(Dynamic::from(count as crate::INT)),
        Err(error) => crate::result::err(made(error)),
    };
    Dynamic::from(vec![result, Dynamic::from(ImmutableString::from(text))])
}

/// Bind `stdin()`, `stdout()` and their methods.
fn bind_streams(runtime: &mut host::runtime::Builder) {
    use std::io::{BufRead as _, Read as _, Write as _};

    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| {
        runtime
            .bind(format!("{}::{path}", super::ROOT), callback)
            .unwrap_or_else(|_| panic!("the std bindings must retain {path}"));
    };
    bind("io::stdin", |_| Ok(Dynamic::from(Stdin)));
    bind("io::stdout", |_| Ok(Dynamic::from(Stdout)));
    bind("io::Stdin::lock", |_| Ok(Dynamic::from(Stdin)));
    bind("io::Stdout::lock", |_| Ok(Dynamic::from(Stdout)));
    bind("io::Stdin::read_line", |_| {
        Ok(appended(|text| std::io::stdin().lock().read_line(text)))
    });
    bind("io::Stdin::read_to_string", |_| {
        Ok(appended(|text| {
            std::io::stdin().lock().read_to_string(text)
        }))
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

fn read<T: Clone + 'static>(value: &Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>> {
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

/// Bind the types' items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| {
        runtime
            .bind(format!("{}::{path}", super::ROOT), callback)
            .unwrap_or_else(|_| panic!("the std bindings must retain {path}"));
    };
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
        let kind = read::<ErrorKind>(args[0], "ErrorKind")?;
        let text = read::<ImmutableString>(args[1], "String")?;
        Ok(made(std::io::Error::new(kind, text.to_string())))
    });
    bind("io::Error::other", |args| {
        let text = read::<ImmutableString>(args[0], "String")?;
        Ok(made(std::io::Error::other(text.to_string())))
    });
    bind("io::Error::from", |args| {
        Ok(made(std::io::Error::from(read::<ErrorKind>(
            args[0],
            "ErrorKind",
        )?)))
    });
    bind("io::Error::kind", |args| {
        Ok(Dynamic::from(
            read::<Error>(args[0], "io::Error")?.get().kind(),
        ))
    });
    bind("io::Error::to_string", |args| {
        Ok(Dynamic::from(ImmutableString::from(
            read::<Error>(args[0], "io::Error")?.to_string(),
        )))
    });
    bind("io::ErrorKind::to_string", |args| {
        Ok(Dynamic::from(ImmutableString::from(
            read::<ErrorKind>(args[0], "ErrorKind")?.to_string(),
        )))
    });
    bind_streams(runtime);
}
