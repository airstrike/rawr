//! std's `thread::Builder` for Rawr code: Rust's own builder, whose name
//! and stack size the thread its `spawn` starts has
//! (`resolve::lower_nominal::thread`).

use std::sync::{Arc, Mutex, PoisonError};

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, ImmutableString, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 1] = [Held::Value(held::Value {
    std: "std::thread::Builder",
    lent: &[],
    also: &[],
    register: |builder, path, capabilities| {
        builder.type_::<Builder>(path, &[], capabilities).map(drop)
    },
    crosses: false,
    runs: &[
        capability::Kind::Debug,
        capability::Kind::Send,
        capability::Kind::Static,
    ],
})];

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
/// `io::Result<JoinHandle<T>>`. Only rawr with its `sync` feature starts
/// threads.
#[cfg(feature = "sync")]
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

/// Register `Debug` for the type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Builder>();
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
    bind("thread::Builder::stack_size", |args| {
        let (builder, name) = Builder::take(args[0])?;
        let size = usize::try_from(args[1].as_int().unwrap_or(0)).unwrap_or(usize::MAX);
        Ok(Dynamic::from(Builder::new(Configured {
            builder: builder.stack_size(size),
            name,
        })))
    });
}
