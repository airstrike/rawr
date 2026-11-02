//! std's `fs` functions for Rawr code (`resolve::std_reexport` sends
//! `std::fs` here). Each is Rust's own, called on the program's paths, and
//! returns its `io::Result`. Each first asks the calling program's
//! [`Access`](crate::Access) for the path, which refuses as the operating
//! system would: `PermissionDenied`.

use std::path::{Path, PathBuf};

use crate::host::runtime::Call;
use crate::host::{self, facade, item, signature, type_ref};
use crate::{Dynamic, EvalAltResult, ImmutableString, Position};

/// What a function needs of each path it names, in order.
#[derive(Clone, Copy)]
enum Need {
    Read,
    Write,
}

/// A function of `std::fs`: its name, what it needs of its paths, whether
/// its last parameter is the contents to write, and what its `Ok` holds.
struct Function {
    name: &'static str,
    paths: &'static [Need],
    contents: bool,
    output: fn() -> type_ref::Type,
    call: fn(&[PathBuf], Option<&[u8]>) -> std::io::Result<Dynamic>,
}

fn unit() -> type_ref::Type {
    type_ref::Type::unit()
}

fn string() -> type_ref::Type {
    type_ref::Type::builtin(type_ref::Builtin::String)
}

fn text() -> type_ref::Type {
    type_ref::Type::reference(false, string())
}

fn bytes() -> type_ref::Type {
    type_ref::Type::slice(type_ref::Type::builtin(type_ref::Builtin::U8))
}

fn boolean() -> type_ref::Type {
    type_ref::Type::builtin(type_ref::Builtin::Bool)
}

fn count() -> type_ref::Type {
    type_ref::Type::builtin(type_ref::Builtin::U64)
}

fn done(result: std::io::Result<()>) -> std::io::Result<Dynamic> {
    result.map(|()| Dynamic::UNIT)
}

const FUNCTIONS: &[Function] = &[
    Function {
        name: "read",
        paths: &[Need::Read],
        contents: false,
        output: bytes,
        call: |paths, _| {
            std::fs::read(&paths[0]).map(|bytes| {
                Dynamic::from_array(
                    bytes
                        .into_iter()
                        .map(|byte| Dynamic::from_int(crate::INT::from(byte)))
                        .collect(),
                )
            })
        },
    },
    Function {
        name: "read_to_string",
        paths: &[Need::Read],
        contents: false,
        output: string,
        call: |paths, _| {
            std::fs::read_to_string(&paths[0])
                .map(|text| Dynamic::from(ImmutableString::from(text)))
        },
    },
    Function {
        name: "write",
        paths: &[Need::Write],
        contents: true,
        output: unit,
        call: |paths, contents| done(std::fs::write(&paths[0], contents.unwrap_or_default())),
    },
    Function {
        name: "exists",
        paths: &[Need::Read],
        contents: false,
        output: boolean,
        call: |paths, _| std::fs::exists(&paths[0]).map(Dynamic::from_bool),
    },
    Function {
        name: "create_dir",
        paths: &[Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::create_dir(&paths[0])),
    },
    Function {
        name: "create_dir_all",
        paths: &[Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::create_dir_all(&paths[0])),
    },
    Function {
        name: "remove_file",
        paths: &[Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::remove_file(&paths[0])),
    },
    Function {
        name: "remove_dir",
        paths: &[Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::remove_dir(&paths[0])),
    },
    Function {
        name: "remove_dir_all",
        paths: &[Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::remove_dir_all(&paths[0])),
    },
    Function {
        name: "rename",
        paths: &[Need::Write, Need::Write],
        contents: false,
        output: unit,
        call: |paths, _| done(std::fs::rename(&paths[0], &paths[1])),
    },
    Function {
        name: "copy",
        paths: &[Need::Read, Need::Write],
        contents: false,
        output: count,
        call: |paths, _| {
            std::fs::copy(&paths[0], &paths[1]).map(|copied| {
                Dynamic::from_int(crate::INT::try_from(copied).unwrap_or(crate::INT::MAX))
            })
        },
    },
];

/// Every combination of `options`, `length` long: each overload's
/// parameters, as Rust's `AsRef<Path>` and `AsRef<[u8]>` take any of them.
fn combinations(options: &[Vec<type_ref::Type>]) -> Vec<Vec<type_ref::Type>> {
    options.iter().fold(vec![Vec::new()], |made, choices| {
        made.iter()
            .flat_map(|prefix| {
                choices.iter().map(move |choice| {
                    let mut next = prefix.clone();
                    next.push(choice.clone());
                    next
                })
            })
            .collect()
    })
}

/// Declare the module's items in the std facade, under `fs`.
pub(crate) fn declare(builder: &mut facade::Builder) -> Result<(), host::Error> {
    use type_ref::Type;

    builder.module("fs")?;
    let path = || -> Result<Vec<Type>, host::Error> {
        Ok(vec![text(), string(), Type::registered(super::path::PATH)?])
    };
    for function in FUNCTIONS {
        let mut options = Vec::new();
        for _ in function.paths {
            options.push(path()?);
        }
        if function.contents {
            options.push(vec![text(), string(), bytes()]);
        }
        for parameters in combinations(&options) {
            let mut signature = signature::Builder::new();
            for parameter in parameters {
                signature.parameter(parameter);
            }
            signature.output(Type::result(
                (function.output)(),
                Type::registered("io::Error")?,
            ));
            builder.item(item::Definition::function(
                format!("fs::{}", function.name),
                signature.finish(),
            )?)?;
        }
    }
    Ok(())
}

fn error(message: String) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

/// A path argument: a `String` or a `PathBuf`, as Rust's `AsRef<Path>`.
fn path(value: &Dynamic) -> Result<PathBuf, Box<EvalAltResult>> {
    if let Some(text) = value.read_lock::<ImmutableString>() {
        return Ok(PathBuf::from(text.as_str()));
    }
    value
        .read_lock::<PathBuf>()
        .map(|path| path.clone())
        .ok_or_else(|| error(format!("expected a path, found `{}`", value.type_name())))
}

/// The contents argument: a `String` or bytes, as Rust's `AsRef<[u8]>`.
fn contents(value: &Dynamic) -> Result<Vec<u8>, Box<EvalAltResult>> {
    if let Some(text) = value.read_lock::<ImmutableString>() {
        return Ok(text.as_bytes().to_vec());
    }
    let bytes = value
        .read_lock::<crate::Array>()
        .ok_or_else(|| error(format!("expected bytes, found `{}`", value.type_name())))?;
    bytes
        .iter()
        .map(|byte| {
            byte.as_int()
                .ok()
                .and_then(|byte| u8::try_from(byte).ok())
                .ok_or_else(|| error(format!("expected a byte, found `{byte}`")))
        })
        .collect()
}

/// Call `function` as `call`'s program may: each path asked of its
/// access first.
fn invoke(
    function: &Function,
    call: &Call<'_>,
    args: &mut [&mut Dynamic],
) -> Result<Dynamic, Box<EvalAltResult>> {
    let paths = function
        .paths
        .iter()
        .zip(args.iter())
        .map(|(_, value)| path(value))
        .collect::<Result<Vec<_>, _>>()?;
    let written = match function.contents {
        true => Some(contents(args[function.paths.len()])?),
        false => None,
    };
    let permitted = function.paths.iter().zip(&paths).try_for_each(
        |(need, path): (&Need, &PathBuf)| match need {
            Need::Read => call.access().read(Path::new(path)),
            Need::Write => call.access().write(Path::new(path)),
        },
    );
    Ok(
        match permitted.and_then(|()| (function.call)(&paths, written.as_deref())) {
            Ok(value) => crate::result::ok(value),
            Err(failure) => crate::result::err(super::io::made(failure)),
        },
    )
}

/// Bind the module's items in the std facade.
pub(crate) fn bind(runtime: &mut host::runtime::Builder) {
    for function in FUNCTIONS {
        runtime
            .bind_with_context(
                format!("{}::fs::{}", super::ROOT, function.name),
                move |call, args| invoke(function, call, args),
            )
            .unwrap_or_else(|_| panic!("the std bindings must retain fs::{}", function.name));
    }
}
