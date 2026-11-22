//! std's `fs::File` and `fs::OpenOptions` for Rawr code. A file is Rust's
//! own `File`, which the program reads, writes and seeks through std's
//! `Read`, `Write` and `Seek`. Opening one first asks the calling
//! program's [`Access`](crate::Access) for its path, as each `fs` function
//! does: to read it, and to write it where the file is opened to write,
//! append, truncate or create.

use std::io::{Read as _, Seek as _, Write as _};
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use super::held::{self, Held};
use crate::host::runtime::Call;
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::fs::File",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<File>(path, parameters, capabilities)
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
        std: "std::fs::OpenOptions",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<OpenOptions>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
];

/// std's `File`, which is not `Clone`: a Rawr value is a handle to the
/// file's one place, which its reads, writes and seeks move through. The
/// file closes when the program's last handle to it goes.
#[derive(Clone)]
pub(crate) struct File(Arc<Mutex<std::fs::File>>);

impl File {
    fn new(file: std::fs::File) -> Self {
        Self(Arc::new(Mutex::new(file)))
    }

    /// The file, lent to `visit`.
    fn with<R>(&self, visit: impl FnOnce(&mut std::fs::File) -> R) -> R {
        visit(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

impl std::fmt::Debug for File {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|file| std::fmt::Debug::fmt(file, formatter))
    }
}

/// std's `OpenOptions`, with what it opens a file for, which std keeps
/// private and the program's access is asked for.
#[derive(Clone)]
pub(crate) struct OpenOptions {
    options: std::fs::OpenOptions,
    read: bool,
    changes: Changes,
}

/// What an `OpenOptions` lets an open file change.
#[derive(Clone, Copy, Default)]
struct Changes {
    write: bool,
    append: bool,
    truncate: bool,
    create: bool,
    create_new: bool,
}

impl Changes {
    /// Whether a file opened so is written, which asks for the path's write.
    const fn writes(self) -> bool {
        self.write || self.append || self.truncate || self.create || self.create_new
    }
}

impl std::fmt::Debug for OpenOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self.options, formatter)
    }
}

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<File>();
    crate::host::debug::register::<OpenOptions>();
}

fn error(message: impl Into<String>) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into().into(), Position::NONE).into()
}

fn file(value: &Dynamic) -> Result<File, Box<EvalAltResult>> {
    value
        .read_lock::<File>()
        .map(|file| file.clone())
        .ok_or_else(|| error(format!("expected `File`, found `{}`", value.type_name())))
}

fn flag(value: &Dynamic) -> Result<bool, Box<EvalAltResult>> {
    value.as_bool().map_err(|_| error("expected a `bool`"))
}

fn int(value: u64) -> Dynamic {
    Dynamic::from_int(crate::INT::try_from(value).unwrap_or(crate::INT::MAX))
}

/// An `io::Result` of `result`, its value made by `made`.
fn io_result<T>(result: std::io::Result<T>, made: impl FnOnce(T) -> Dynamic) -> Dynamic {
    match result {
        Ok(value) => crate::result::ok(made(value)),
        Err(failure) => crate::result::err(super::io::made(failure)),
    }
}

fn unit(result: std::io::Result<()>) -> Dynamic {
    io_result(result, |()| Dynamic::UNIT)
}

/// Open the file at `path` as `call`'s program may: its access asked for
/// the path first, to read it where `read` and to write it where
/// `changes` write it.
fn open(
    call: &Call<'_>,
    path: &Path,
    read: bool,
    changes: Changes,
    open: impl FnOnce() -> std::io::Result<std::fs::File>,
) -> Dynamic {
    let permitted = match (read, changes.writes()) {
        (_, true) => call.access().write(path),
        (true, false) => call.access().read(path),
        // Neither: std's `open` fails for its options, and asks nothing.
        (false, false) => Ok(()),
    };
    io_result(permitted.and_then(|()| open()), |file| {
        Dynamic::from(File::new(file))
    })
}

/// Change the options at `args[0]` by `change`, and give them back, as a
/// builder call's `&mut OpenOptions`.
fn building(
    args: &mut [&mut Dynamic],
    change: impl FnOnce(&mut OpenOptions, bool),
) -> Result<Dynamic, Box<EvalAltResult>> {
    let on = flag(args[1])?;
    let actual = args[0].type_name();
    let mut options = args[0]
        .write_lock::<OpenOptions>()
        .ok_or_else(|| error(format!("expected `OpenOptions`, found `{actual}`")))?;
    change(&mut options, on);
    drop(options);
    Ok(args[0].clone())
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    binds.with_context("fs::File::open", |call, args| {
        let path = super::path::path_like(args[0])?;
        Ok(open(call, &path, true, Changes::default(), || {
            std::fs::File::open(&path)
        }))
    });
    binds.with_context("fs::File::create", |call, args| {
        let path = super::path::path_like(args[0])?;
        let changes = Changes {
            write: true,
            truncate: true,
            create: true,
            ..Changes::default()
        };
        Ok(open(call, &path, false, changes, || {
            std::fs::File::create(&path)
        }))
    });
    binds.with_context("fs::File::create_new", |call, args| {
        let path = super::path::path_like(args[0])?;
        let changes = Changes {
            write: true,
            create_new: true,
            ..Changes::default()
        };
        Ok(open(call, &path, true, changes, || {
            std::fs::File::create_new(&path)
        }))
    });
    binds.with_context("fs::OpenOptions::open", |call, args| {
        let path = super::path::path_like(args[1])?;
        let actual = args[0].type_name();
        let options = args[0]
            .read_lock::<OpenOptions>()
            .map(|options| options.clone())
            .ok_or_else(|| error(format!("expected `OpenOptions`, found `{actual}`")))?;
        Ok(open(call, &path, options.read, options.changes, || {
            options.options.open(&path)
        }))
    });
    bind_options(binds);
    bind_file(binds);
}

/// Bind `OpenOptions`'s constructors and its builder calls.
fn bind_options(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    let new = |_: &mut [&mut Dynamic]| {
        Ok(Dynamic::from(OpenOptions {
            options: std::fs::OpenOptions::new(),
            read: false,
            changes: Changes::default(),
        }))
    };
    bind("fs::OpenOptions::new", new);
    bind("fs::File::options", new);
    bind("fs::OpenOptions::read", |args| {
        building(args, |options, on| {
            options.options.read(on);
            options.read = on;
        })
    });
    bind("fs::OpenOptions::write", |args| {
        building(args, |options, on| {
            options.options.write(on);
            options.changes.write = on;
        })
    });
    bind("fs::OpenOptions::append", |args| {
        building(args, |options, on| {
            options.options.append(on);
            options.changes.append = on;
        })
    });
    bind("fs::OpenOptions::truncate", |args| {
        building(args, |options, on| {
            options.options.truncate(on);
            options.changes.truncate = on;
        })
    });
    bind("fs::OpenOptions::create", |args| {
        building(args, |options, on| {
            options.options.create(on);
            options.changes.create = on;
        })
    });
    bind("fs::OpenOptions::create_new", |args| {
        building(args, |options, on| {
            options.options.create_new(on);
            options.changes.create_new = on;
        })
    });
}

/// Bind a file's own methods and its `Read`, `Write` and `Seek`.
fn bind_file(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("fs::File::metadata", |args| {
        Ok(io_result(
            file(args[0])?.with(|file| file.metadata()),
            super::metadata::made,
        ))
    });
    bind("fs::File::sync_all", |args| {
        Ok(unit(file(args[0])?.with(|file| file.sync_all())))
    });
    bind("fs::File::sync_data", |args| {
        Ok(unit(file(args[0])?.with(|file| file.sync_data())))
    });
    bind("fs::File::set_len", |args| {
        let size = args[1]
            .as_int()
            .ok()
            .and_then(|size| u64::try_from(size).ok())
            .ok_or_else(|| error("expected a `u64` size"))?;
        Ok(unit(file(args[0])?.with(|file| file.set_len(size))))
    });
    bind("fs::File::read_to_string", |args| {
        let opened = file(args[0])?;
        super::io::appended(args[1], |text| {
            opened.with(|file| file.read_to_string(text))
        })
    });
    bind("fs::File::read_to_end", |args| {
        let opened = file(args[0])?;
        let mut buffer = super::fs::contents(args[1])?;
        let read = opened.with(|file| file.read_to_end(&mut buffer));
        Ok(Dynamic::from(vec![
            io_result(read, |count| int(count as u64)),
            Dynamic::from(
                buffer
                    .iter()
                    .map(|byte| Dynamic::from_int(crate::INT::from(*byte)))
                    .collect::<crate::Array>(),
            ),
        ]))
    });
    bind("fs::File::write", |args| {
        let written = super::fs::contents(args[1])?;
        Ok(io_result(
            file(args[0])?.with(|file| file.write(&written)),
            |count| int(count as u64),
        ))
    });
    bind("fs::File::write_all", |args| {
        let written = super::fs::contents(args[1])?;
        Ok(unit(file(args[0])?.with(|file| file.write_all(&written))))
    });
    bind("fs::File::write_fmt", |args| {
        let text = args[1]
            .read_lock::<ImmutableString>()
            .map(|text| text.to_string())
            .ok_or_else(|| error("expected `Arguments`"))?;
        Ok(unit(
            file(args[0])?.with(|file| file.write_all(text.as_bytes())),
        ))
    });
    bind("fs::File::flush", |args| {
        Ok(unit(file(args[0])?.with(|file| file.flush())))
    });
    bind("fs::File::seek", |args| {
        let from = args[1]
            .read_lock::<std::io::SeekFrom>()
            .map(|from| *from)
            .ok_or_else(|| error("expected `SeekFrom`"))?;
        Ok(io_result(file(args[0])?.with(|file| file.seek(from)), int))
    });
    bind("fs::File::stream_position", |args| {
        Ok(io_result(
            file(args[0])?.with(|file| file.stream_position()),
            int,
        ))
    });
    bind("fs::File::rewind", |args| {
        Ok(unit(file(args[0])?.with(|file| file.rewind())))
    });
}
