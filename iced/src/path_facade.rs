//! std's `path` and `ffi` types for Rawr code (`resolve::std_reexport`
//! sends `std::path` and `std::ffi` here). Each value is Rust's own
//! `PathBuf` or `OsString`. A borrowed `Path` or `OsStr` is the same value,
//! as Rawr code has no borrows of its own.

use std::ffi::OsString;
use std::path::PathBuf;

use rawr::host::{self, facade, item, signature, type_ref};
use rawr::{capability, receiver, Dynamic, EvalAltResult, ImmutableString, Position};

const CAPABILITIES: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

const PATH: &str = "path::PathBuf";
const OS: &str = "ffi::OsString";

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    rawr::host::debug::register::<PathBuf>();
    rawr::host::debug::register::<OsString>();
}

/// Declare the types and their items in rawr-iced's facade, under `path`
/// and `ffi`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::{Builtin, Type};

    builder.module("path")?;
    builder.module("ffi")?;
    builder.default_value_type::<PathBuf>(PATH, &[], CAPABILITIES)?;
    builder.default_value_type::<OsString>(OS, &[], CAPABILITIES)?;
    builder.alias("path::Path", PATH)?;
    builder.alias("ffi::OsStr", OS)?;

    let string = || Type::builtin(Builtin::String);
    let path = || Type::registered(PATH);
    let os = || Type::registered(OS);
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
    for (owner, made) in [(PATH, path()?), (OS, os()?)] {
        function(format!("{owner}::new"), vec![], made.clone())?;
        function(format!("{owner}::default"), vec![], made.clone())?;
        function(format!("{owner}::from"), vec![string()], made)?;
    }
    let mut method = |path: String, mode: receiver::Mode, inputs: Vec<Type>, output: Type| {
        builder.item(item::Definition::method(path, mode, signature(inputs, output))?)?;
        Ok::<_, host::Error>(())
    };
    use receiver::Mode::{Mutable, Shared};
    for input in [string(), path()?] {
        method(format!("{PATH}::push"), Mutable, vec![input.clone()], Type::unit())?;
        method(format!("{PATH}::join"), Shared, vec![input], path()?)?;
    }
    method(format!("{PATH}::parent"), Shared, vec![], Type::option(path()?))?;
    for name in ["file_name", "file_stem", "extension"] {
        method(format!("{PATH}::{name}"), Shared, vec![], Type::option(os()?))?;
    }
    for name in ["to_path_buf", "to_owned"] {
        method(format!("{PATH}::{name}"), Shared, vec![], path()?)?;
    }
    method(format!("{PATH}::as_os_str"), Shared, vec![], os()?)?;
    method(format!("{OS}::to_os_string"), Shared, vec![], os()?)?;
    method(format!("{OS}::to_owned"), Shared, vec![], os()?)?;
    for owner in [PATH, OS] {
        method(format!("{owner}::to_str"), Shared, vec![], Type::option(string()))?;
        method(format!("{owner}::to_string_lossy"), Shared, vec![], string())?;
    }
    // `path.display()` is the text `{}` prints, which `to_string` reads.
    method(format!("{PATH}::display"), Shared, vec![], string())?;
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

/// A `String` or a `PathBuf` argument, as Rust's `AsRef<Path>`.
fn path_like(value: &Dynamic) -> Result<PathBuf, Box<EvalAltResult>> {
    if let Some(text) = value.read_lock::<ImmutableString>() {
        return Ok(PathBuf::from(text.as_str()));
    }
    read::<PathBuf>(value, "PathBuf")
}

fn text(value: &str) -> Dynamic {
    Dynamic::from(ImmutableString::from(value))
}

fn os_text(value: Option<&std::ffi::OsStr>) -> Dynamic {
    rawr::option::from(value.map(std::ffi::OsStr::to_os_string))
}

/// Bind the types' items in rawr-iced's facade.
pub(crate) fn bind(rawr: &mut host::runtime::Builder) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| {
        rawr.bind(format!("rawr_iced::{path}"), callback)
            .unwrap_or_else(|_| panic!("the rawr_iced facade must retain {path}"));
    };
    bind("path::PathBuf::new", |_| Ok(Dynamic::from(PathBuf::new())));
    bind("path::PathBuf::default", |_| Ok(Dynamic::from(PathBuf::new())));
    bind("path::PathBuf::from", |args| Ok(Dynamic::from(path_like(args[0])?)));
    bind("ffi::OsString::new", |_| Ok(Dynamic::from(OsString::new())));
    bind("ffi::OsString::default", |_| Ok(Dynamic::from(OsString::new())));
    bind("ffi::OsString::from", |args| {
        let text = read::<ImmutableString>(args[0], "String")?;
        Ok(Dynamic::from(OsString::from(text.as_str())))
    });
    bind("path::PathBuf::push", |args| {
        let pushed = path_like(args[1])?;
        let mut path = args[0]
            .write_lock::<PathBuf>()
            .ok_or_else(|| error("expected `PathBuf`"))?;
        path.push(pushed);
        Ok(Dynamic::UNIT)
    });
    bind("path::PathBuf::join", |args| {
        let joined = path_like(args[1])?;
        Ok(Dynamic::from(read::<PathBuf>(args[0], "PathBuf")?.join(joined)))
    });
    bind("path::PathBuf::parent", |args| {
        let path = read::<PathBuf>(args[0], "PathBuf")?;
        Ok(rawr::option::from(path.parent().map(std::path::Path::to_path_buf)))
    });
    bind("path::PathBuf::file_name", |args| {
        Ok(os_text(read::<PathBuf>(args[0], "PathBuf")?.file_name()))
    });
    bind("path::PathBuf::file_stem", |args| {
        Ok(os_text(read::<PathBuf>(args[0], "PathBuf")?.file_stem()))
    });
    bind("path::PathBuf::extension", |args| {
        Ok(os_text(read::<PathBuf>(args[0], "PathBuf")?.extension()))
    });
    bind("path::PathBuf::to_path_buf", |args| {
        Ok(Dynamic::from(read::<PathBuf>(args[0], "PathBuf")?))
    });
    bind("path::PathBuf::to_owned", |args| {
        Ok(Dynamic::from(read::<PathBuf>(args[0], "PathBuf")?))
    });
    bind("path::PathBuf::as_os_str", |args| {
        Ok(Dynamic::from(read::<PathBuf>(args[0], "PathBuf")?.into_os_string()))
    });
    bind("path::PathBuf::to_str", |args| {
        let path = read::<PathBuf>(args[0], "PathBuf")?;
        Ok(rawr::option::from(path.to_str().map(ImmutableString::from)))
    });
    bind("path::PathBuf::to_string_lossy", |args| {
        Ok(text(&read::<PathBuf>(args[0], "PathBuf")?.to_string_lossy()))
    });
    bind("path::PathBuf::display", |args| {
        Ok(text(&read::<PathBuf>(args[0], "PathBuf")?.display().to_string()))
    });
    bind("ffi::OsString::to_os_string", |args| {
        Ok(Dynamic::from(read::<OsString>(args[0], "OsString")?))
    });
    bind("ffi::OsString::to_owned", |args| {
        Ok(Dynamic::from(read::<OsString>(args[0], "OsString")?))
    });
    bind("ffi::OsString::to_str", |args| {
        let os = read::<OsString>(args[0], "OsString")?;
        Ok(rawr::option::from(os.to_str().map(ImmutableString::from)))
    });
    bind("ffi::OsString::to_string_lossy", |args| {
        Ok(text(&read::<OsString>(args[0], "OsString")?.to_string_lossy()))
    });
}
