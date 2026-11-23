//! std's `thread` types for Rawr code: Rust's own `Builder`, whose name
//! and stack size the thread its `spawn` starts has
//! (`resolve::lower_nominal::thread`), and Rust's own `AccessError`, which
//! a key's `try_with` gives once the thread's values are ending
//! (`eval::thread_local`).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::AccessError;

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 2] = [
    Held::Value(held::Value {
        std: "std::thread::Builder",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<Builder>(path, parameters, capabilities)
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
        std: "std::thread::AccessError",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .value_type::<AccessError>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: true,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Send,
            capability::Kind::Static,
            capability::Kind::Error,
        ],
    }),
];

/// std's `AccessError`, which no code outside std makes: std's own, as
/// the access of a key during its own value's drop gives it, made once
/// on a thread of its own.
pub(crate) fn access_error() -> Result<Dynamic, Box<EvalAltResult>> {
    static MADE: OnceLock<AccessError> = OnceLock::new();
    struct Ending;
    impl Drop for Ending {
        fn drop(&mut self) {
            if let Err(made) = ENDING.try_with(|_| ()) {
                let _ = MADE.set(made);
            }
        }
    }
    std::thread_local! {
        static ENDING: Ending = const { Ending };
    }
    if MADE.get().is_none() {
        let _ = std::thread::spawn(|| ENDING.with(|_| ())).join();
    }
    MADE.get()
        .copied()
        .map(Dynamic::from)
        .ok_or_else(|| error("std's thread-local destructors did not run"))
}

/// Rust's `std::thread::Builder`, which is not `Clone`: a Rawr value is a
/// handle to the builder's one place, which each method that takes the
/// builder empties, as Rust moves it. The name it gives its thread is kept
/// beside it, since std's builder doesn't tell it.
#[derive(Clone)]
pub(crate) struct Builder(Arc<Mutex<Option<Configured>>>);

struct Configured {
    builder: std::thread::Builder,
    name: Option<String>,
}

impl Builder {
    fn new(configured: Configured) -> Self {
        Self(Arc::new(Mutex::new(Some(configured))))
    }

    /// The builder and the name of its thread, taken from its place.
    fn take(value: &Dynamic) -> Result<(std::thread::Builder, Option<String>), Box<EvalAltResult>> {
        let configured = value
            .read_lock::<Self>()
            .and_then(|builder| {
                builder
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take()
            })
            .ok_or_else(|| error("a `thread::Builder` was used after it moved"))?;
        Ok((configured.builder, configured.name))
    }
}

impl std::fmt::Debug for Builder {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &*self.0.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(configured) => configured.builder.fmt(formatter),
            None => formatter.write_str("Builder { .. }"),
        }
    }
}

/// `builder.spawn(f)` of the builder `value`: the thread `start` starts
/// with std's builder and the name it gives the thread, as std's
/// `io::Result<JoinHandle<T>>`.
pub(crate) fn spawn(
    value: &Dynamic,
    start: impl FnOnce(
        std::thread::Builder,
        Option<String>,
    ) -> Result<std::io::Result<Dynamic>, Box<EvalAltResult>>,
) -> Result<Dynamic, Box<EvalAltResult>> {
    let (builder, name) = Builder::take(value)?;
    Ok(match start(builder, name)? {
        Ok(handle) => crate::result::ok(handle),
        Err(failed) => crate::result::err(super::io::made(failed)),
    })
}

fn error(message: &str) -> Box<EvalAltResult> {
    EvalAltResult::ErrorRuntime(message.into(), Position::NONE).into()
}

/// Register `Debug` for each type, and `Display` for `AccessError`.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Builder>();
    crate::host::debug::register::<AccessError>();
    crate::host::display::register::<AccessError>();
}

/// Bind the type's items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("thread::Builder::new", |_| {
        Ok(Dynamic::from(Builder::new(Configured {
            builder: std::thread::Builder::new(),
            name: None,
        })))
    });
    bind("thread::Builder::name", |args| {
        let (builder, _) = Builder::take(args[0])?;
        let name = args[1]
            .read_lock::<ImmutableString>()
            .map(|name| name.to_string())
            .ok_or_else(|| error("a thread's name is no `String`"))?;
        Ok(Dynamic::from(Builder::new(Configured {
            builder: builder.name(name.clone()),
            name: Some(name),
        })))
    });
    bind("thread::AccessError::to_string", |args| {
        args[0]
            .read_lock::<AccessError>()
            .map(|made| Dynamic::from(ImmutableString::from(made.to_string())))
            .ok_or_else(|| error("an `AccessError` is not std's at runtime"))
    });
    bind("thread::Builder::stack_size", |args| {
        let (builder, name) = Builder::take(args[0])?;
        let size = usize::try_from(args[1].as_int().unwrap_or(0)).unwrap_or(usize::MAX);
        Ok(Dynamic::from(Builder::new(Configured {
            builder: builder.stack_size(size),
            name,
        })))
    });
}
