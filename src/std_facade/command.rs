//! std's `process::Command`, the `process::ExitStatus` its run ends with
//! and the `process::Output` it collects, for Rawr code. A command is Rust's own `Command`, which the
//! program's builder calls change in its one place, so `cmd.arg(..)` and
//! `Command::new(..).arg(..).status()` build the same command std's do. A
//! run asks the calling program's [`Access`](crate::Access) for the
//! process, which refuses as the operating system would:
//! `PermissionDenied`. The child writes to the process's own stdout and
//! stderr, after what the program printed.

use std::process::{ExitStatus, Output};
use std::sync::{Arc, Mutex, PoisonError};

use super::held::{self, Held};
use crate::host::runtime::Call;
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 3] = [
    Held::Value(held::Value {
        std: "std::process::Command",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<Command>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::process::ExitStatus",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .default_value_type::<ExitStatus>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Default,
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::process::Output",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .value_type::<Output>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
];

/// std's `Command`, which is not `Clone`: a Rawr value is a handle to the
/// command's one place, which each builder call changes.
#[derive(Clone)]
pub(crate) struct Command(Arc<Mutex<std::process::Command>>);

impl Command {
    /// The command, lent to `visit`.
    fn with<R>(&self, visit: impl FnOnce(&mut std::process::Command) -> R) -> R {
        visit(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl std::fmt::Debug for Command {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|command| std::fmt::Debug::fmt(command, formatter))
    }
}

/// Register `Debug` for each type, and `ExitStatus`'s `Display`.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Command>();
    crate::host::debug::register::<ExitStatus>();
    crate::host::debug::register::<Output>();
    crate::host::display::register::<ExitStatus>();
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn command(value: &Dynamic) -> Result<Command, Box<EvalAltResult>> {
    value
        .read_lock::<Command>()
        .map(|command| command.clone())
        .ok_or_else(|| error(format!("expected `Command`, found `{}`", value.type_name())))
}

fn exit_status(value: &Dynamic) -> Result<ExitStatus, Box<EvalAltResult>> {
    value
        .read_lock::<ExitStatus>()
        .map(|status| *status)
        .ok_or_else(|| {
            error(format!(
                "expected `ExitStatus`, found `{}`",
                value.type_name()
            ))
        })
}

/// The items of the `IntoIterator` at `value`: the elements of a `Vec`, a
/// slice or an iterator (an array at runtime), an `Option`'s value, or a
/// path's components.
fn items(value: &Dynamic) -> Result<Vec<Dynamic>, Box<EvalAltResult>> {
    if let Some(option) = crate::option::read(value) {
        return Ok(option.into_iter().collect());
    }
    if let Some(path) = value.read_lock::<std::path::PathBuf>() {
        return Ok(path
            .iter()
            .map(|component| Dynamic::from(component.to_os_string()))
            .collect());
    }
    value
        .as_array_ref()
        .map(|items| items.to_vec())
        .map_err(|found| error(format!("expected the arguments' items, found `{found}`")))
}

/// Change the command at `args[0]` by `change`, and give the command back,
/// as a builder call's `&mut Command`.
fn building(
    args: &mut [&mut Dynamic],
    change: impl FnOnce(&mut std::process::Command, &[&mut Dynamic]) -> Result<(), Box<EvalAltResult>>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let (receiver, arguments) = args
        .split_first_mut()
        .ok_or_else(|| error("a builder call takes its command"))?;
    let built = command(receiver)?;
    built.with(|command| change(command, arguments))?;
    Ok((**receiver).clone())
}

/// Run the command at `value` to its end by `run`, as `call`'s program
/// may: what `run` gives, or the `io::Error` its start failed with. What the
/// program printed is flushed first, so the child's output comes after it.
fn ran<T: crate::types::dynamic::Variant + Clone>(
    call: &Call<'_>,
    value: &Dynamic,
    run: fn(&mut std::process::Command) -> std::io::Result<T>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    use std::io::Write as _;

    let built = command(value)?;
    let result = if call.access().process() {
        // A failed flush leaves the child's output where it falls.
        let _ = std::io::stdout().flush();
        built.with(run)
    } else {
        Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied))
    };
    Ok(match result {
        Ok(ran) => crate::result::ok(Dynamic::from(ran)),
        Err(failure) => crate::result::err(super::io::made(failure)),
    })
}

fn output(value: &Dynamic) -> Result<Output, Box<EvalAltResult>> {
    value
        .read_lock::<Output>()
        .map(|output| output.clone())
        .ok_or_else(|| error(format!("expected `Output`, found `{}`", value.type_name())))
}

/// `bytes` as a Rawr `Vec<u8>`.
fn bytes(bytes: Vec<u8>) -> Dynamic {
    Dynamic::from_array(
        bytes
            .into_iter()
            .map(|byte| Dynamic::from_int(crate::INT::from(byte)))
            .collect(),
    )
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    binds.with_context("process::Command::status", |call, args| {
        ran(call, args[0], std::process::Command::status)
    });
    // The child's stdout and stderr are collected, its stdin is null.
    binds.with_context("process::Command::output", |call, args| {
        ran(call, args[0], std::process::Command::output)
    });
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("process::Command::new", |args| {
        let program = super::path::os_like(args[0])?;
        Ok(Dynamic::from(Command(Arc::new(Mutex::new(
            std::process::Command::new(program),
        )))))
    });
    bind("process::Command::arg", |args| {
        building(args, |command, arguments| {
            command.arg(super::path::os_like(arguments[0])?);
            Ok(())
        })
    });
    bind("process::Command::args", |args| {
        building(args, |command, arguments| {
            for argument in items(arguments[0])? {
                command.arg(super::path::os_like(&argument)?);
            }
            Ok(())
        })
    });
    bind("process::Command::env", |args| {
        building(args, |command, arguments| {
            command.env(
                super::path::os_like(arguments[0])?,
                super::path::os_like(arguments[1])?,
            );
            Ok(())
        })
    });
    bind("process::Command::env_remove", |args| {
        building(args, |command, arguments| {
            command.env_remove(super::path::os_like(arguments[0])?);
            Ok(())
        })
    });
    bind("process::Command::env_clear", |args| {
        building(args, |command, _| {
            command.env_clear();
            Ok(())
        })
    });
    bind("process::Command::current_dir", |args| {
        building(args, |command, arguments| {
            command.current_dir(super::path::path_like(arguments[0])?);
            Ok(())
        })
    });
    bind("process::Command::get_program", |args| {
        Ok(Dynamic::from(
            command(args[0])?.with(|command| command.get_program().to_os_string()),
        ))
    });
    // `Output`'s public fields, each read by the reader of its name.
    bind("process::Output::status", |args| {
        Ok(Dynamic::from(output(args[0])?.status))
    });
    bind("process::Output::stdout", |args| {
        Ok(bytes(output(args[0])?.stdout))
    });
    bind("process::Output::stderr", |args| {
        Ok(bytes(output(args[0])?.stderr))
    });
    bind("process::ExitStatus::success", |args| {
        Ok(Dynamic::from_bool(exit_status(args[0])?.success()))
    });
    bind("process::ExitStatus::code", |args| {
        Ok(crate::option::from(
            exit_status(args[0])?.code().map(crate::INT::from),
        ))
    });
    bind("process::ExitStatus::to_string", |args| {
        Ok(Dynamic::from(ImmutableString::from(
            exit_status(args[0])?.to_string(),
        )))
    });
}
