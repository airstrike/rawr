//! std's `io::Error` and `io::ErrorKind` for Rawr code
//! (`resolve::std_reexport` sends `std::io` here). An error is Rust's own
//! `std::io::Error`, shared so a Rawr value can be copied. Nothing here
//! reads or writes a file.

use std::io::ErrorKind;
use std::sync::Arc;

use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Dynamic, EvalAltResult, ImmutableString, Position};

const ERROR: &str = "io::Error";
const KIND: &str = "io::ErrorKind";

/// Rust's `std::io::Error`, which is not `Clone`, behind an `Arc`.
#[derive(Clone)]
pub(crate) struct Error(Arc<std::io::Error>);

impl std::fmt::Debug for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, formatter)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&*self.0, formatter)
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
    rawr::host::debug::register::<Error>();
    rawr::host::debug::register::<ErrorKind>();
    rawr::host::display::register::<Error>();
    rawr::host::display::register::<ErrorKind>();
}

/// Declare the types and their items in rawr-iced's facade, under `io`.
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
    function(format!("{ERROR}::new"), vec![kind()?, string()], error()?)?;
    function(format!("{ERROR}::other"), vec![string()], error()?)?;
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
    Ok(())
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

fn made(error: std::io::Error) -> Dynamic {
    Dynamic::from(Error(Arc::new(error)))
}

/// Bind the types' items in rawr-iced's facade.
pub(crate) fn bind(rawr: &mut host::runtime::Builder) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| {
        rawr.bind(format!("rawr_iced::{path}"), callback)
            .unwrap_or_else(|_| panic!("the rawr_iced facade must retain {path}"));
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
        Ok(Dynamic::from(read::<Error>(args[0], "io::Error")?.0.kind()))
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
}
