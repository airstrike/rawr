//! std's `path` and `ffi` types for Rawr code. Each value is Rust's own
//! `PathBuf` or `OsString`. A borrowed `Path` or `OsStr` is the same value,
//! as Rawr code has no borrows of its own.

use std::ffi::OsString;
use std::path::PathBuf;

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// What rawr runs for a path's and an `OsString`'s value.
const RUNS: &[capability::Kind] = &[
    capability::Kind::Default,
    capability::Kind::Debug,
    capability::Kind::Clone,
    capability::Kind::PartialEq,
    capability::Kind::Eq,
    capability::Kind::Send,
    capability::Kind::Static,
];

/// The types the module holds: a `path.display()` is the text it writes.
pub(super) const HELD: [Held; 3] = [
    Held::Value(held::Value {
        std: "std::path::PathBuf",
        parameters: &[],
        lent: &["std::path::Path"],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .default_value_type::<PathBuf>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
    Held::Value(held::Value {
        std: "std::ffi::OsString",
        parameters: &[],
        lent: &["std::ffi::OsStr"],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .default_value_type::<OsString>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: RUNS,
    }),
    Held::Text("std::path::Display"),
];

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<PathBuf>();
    crate::host::debug::register::<OsString>();
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

/// A `String`, an `OsString` or a `PathBuf` argument, as Rust's
/// `AsRef<Path>`.
pub(super) fn path_like(value: &Dynamic) -> Result<PathBuf, Box<EvalAltResult>> {
    if let Some(path) = value.read_lock::<PathBuf>() {
        return Ok(path.clone());
    }
    Ok(PathBuf::from(os_like(value)?))
}

/// A `String`, an `OsString` or a `PathBuf` argument, as Rust's
/// `AsRef<OsStr>`.
fn os_like(value: &Dynamic) -> Result<OsString, Box<EvalAltResult>> {
    if let Some(text) = value.read_lock::<ImmutableString>() {
        return Ok(OsString::from(text.as_str()));
    }
    if let Some(path) = value.read_lock::<PathBuf>() {
        return Ok(path.clone().into_os_string());
    }
    read::<OsString>(value, "OsString")
}

fn text(value: &str) -> Dynamic {
    Dynamic::from(ImmutableString::from(value))
}

/// std's `Cow<str>` of a `to_string_lossy`, as a program's `Cow<str>` is
/// at runtime: borrowed of valid text, owned of a replaced one.
fn lossy(value: std::borrow::Cow<'_, str>) -> Dynamic {
    use crate::resolve::lower_nominal::cow::Value;
    Dynamic::from(match value {
        std::borrow::Cow::Borrowed(borrowed) => Value::Borrowed(text(borrowed)),
        std::borrow::Cow::Owned(owned) => Value::Owned(text(&owned)),
    })
}

fn os_text(value: Option<&std::ffi::OsStr>) -> Dynamic {
    crate::option::from(value.map(std::ffi::OsStr::to_os_string))
}

/// Whether a file system entry is of the kind a query asks for.
type Holds = fn(&std::fs::Metadata) -> bool;

/// What the file system holds at a path: std's `Path::exists`, `is_file`
/// and `is_dir`, each `false` where the path can't be read.
const QUERIES: &[(&str, Holds)] = &[
    ("exists", |_| true),
    ("is_file", std::fs::Metadata::is_file),
    ("is_dir", std::fs::Metadata::is_dir),
];

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    for (name, holds) in QUERIES {
        binds.with_context(&format!("path::PathBuf::{name}"), move |call, args| {
            let path = read::<PathBuf>(args[0], "PathBuf")?;
            let found = call
                .access()
                .read(&path)
                .and_then(|()| std::fs::metadata(&path));
            Ok(Dynamic::from_bool(
                found.is_ok_and(|metadata| holds(&metadata)),
            ))
        });
    }
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("path::PathBuf::new", |args| match args {
        [] => Ok(Dynamic::from(PathBuf::new())),
        [text, ..] => Ok(Dynamic::from(path_like(text)?)),
    });
    bind("path::PathBuf::default", |_| {
        Ok(Dynamic::from(PathBuf::new()))
    });
    bind("path::PathBuf::from", |args| {
        Ok(Dynamic::from(path_like(args[0])?))
    });
    bind("ffi::OsString::new", |_| Ok(Dynamic::from(OsString::new())));
    bind("ffi::OsString::default", |_| {
        Ok(Dynamic::from(OsString::new()))
    });
    bind("ffi::OsString::from", |args| {
        Ok(Dynamic::from(os_like(args[0])?))
    });
    bind("path::PathBuf::push", |args| {
        let pushed = path_like(args[1])?;
        let mut path = args[0]
            .write_lock::<PathBuf>()
            .ok_or_else(|| error("expected `PathBuf`"))?;
        path.push(pushed);
        Ok(Dynamic::UNIT)
    });
    bind("path::PathBuf::set_extension", |args| {
        let extension = os_like(args[1])?;
        let mut path = args[0]
            .write_lock::<PathBuf>()
            .ok_or_else(|| error("expected `PathBuf`"))?;
        Ok(Dynamic::from_bool(path.set_extension(extension)))
    });
    bind("path::PathBuf::set_file_name", |args| {
        let name = os_like(args[1])?;
        let mut path = args[0]
            .write_lock::<PathBuf>()
            .ok_or_else(|| error("expected `PathBuf`"))?;
        path.set_file_name(name);
        Ok(Dynamic::UNIT)
    });
    bind("path::PathBuf::pop", |args| {
        let mut path = args[0]
            .write_lock::<PathBuf>()
            .ok_or_else(|| error("expected `PathBuf`"))?;
        Ok(Dynamic::from_bool(path.pop()))
    });
    bind("path::PathBuf::with_extension", |args| {
        let extension = os_like(args[1])?;
        Ok(Dynamic::from(
            read::<PathBuf>(args[0], "PathBuf")?.with_extension(extension),
        ))
    });
    bind("path::PathBuf::with_file_name", |args| {
        let name = os_like(args[1])?;
        Ok(Dynamic::from(
            read::<PathBuf>(args[0], "PathBuf")?.with_file_name(name),
        ))
    });
    bind("path::PathBuf::starts_with", |args| {
        let base = path_like(args[1])?;
        Ok(Dynamic::from_bool(
            read::<PathBuf>(args[0], "PathBuf")?.starts_with(base),
        ))
    });
    bind("path::PathBuf::ends_with", |args| {
        let child = path_like(args[1])?;
        Ok(Dynamic::from_bool(
            read::<PathBuf>(args[0], "PathBuf")?.ends_with(child),
        ))
    });
    bind("path::PathBuf::is_absolute", |args| {
        Ok(Dynamic::from_bool(
            read::<PathBuf>(args[0], "PathBuf")?.is_absolute(),
        ))
    });
    bind("path::PathBuf::is_relative", |args| {
        Ok(Dynamic::from_bool(
            read::<PathBuf>(args[0], "PathBuf")?.is_relative(),
        ))
    });
    bind("path::PathBuf::has_root", |args| {
        Ok(Dynamic::from_bool(
            read::<PathBuf>(args[0], "PathBuf")?.has_root(),
        ))
    });
    bind("path::PathBuf::as_path", |args| Ok(args[0].clone()));
    bind("path::PathBuf::join", |args| {
        let joined = path_like(args[1])?;
        Ok(Dynamic::from(
            read::<PathBuf>(args[0], "PathBuf")?.join(joined),
        ))
    });
    bind("path::PathBuf::parent", |args| {
        let path = read::<PathBuf>(args[0], "PathBuf")?;
        Ok(crate::option::from(
            path.parent().map(std::path::Path::to_path_buf),
        ))
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
        Ok(Dynamic::from(
            read::<PathBuf>(args[0], "PathBuf")?.into_os_string(),
        ))
    });
    bind("path::PathBuf::to_str", |args| {
        let path = read::<PathBuf>(args[0], "PathBuf")?;
        Ok(crate::option::from(
            path.to_str().map(ImmutableString::from),
        ))
    });
    bind("path::PathBuf::to_string_lossy", |args| {
        Ok(lossy(
            read::<PathBuf>(args[0], "PathBuf")?.to_string_lossy(),
        ))
    });
    bind("path::PathBuf::display", |args| {
        Ok(text(
            &read::<PathBuf>(args[0], "PathBuf")?.display().to_string(),
        ))
    });
    bind("ffi::OsString::to_os_string", |args| {
        Ok(Dynamic::from(read::<OsString>(args[0], "OsString")?))
    });
    bind("ffi::OsString::to_owned", |args| {
        Ok(Dynamic::from(read::<OsString>(args[0], "OsString")?))
    });
    bind("ffi::OsString::to_str", |args| {
        let os = read::<OsString>(args[0], "OsString")?;
        Ok(crate::option::from(os.to_str().map(ImmutableString::from)))
    });
    bind("ffi::OsString::to_string_lossy", |args| {
        Ok(lossy(
            read::<OsString>(args[0], "OsString")?.to_string_lossy(),
        ))
    });
}
