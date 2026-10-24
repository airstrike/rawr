//! tokio's timers and channels for Rawr code in a program without a tokio
//! dependency, as `?play` snippets are (`resolve::std_reexport` sends their
//! `tokio::` paths here). Only `time`, `sync` and `task` items exist, so a
//! snippet cannot name tokio's files, sockets or processes.
//!
//! A future runs inside a tokio runtime. While a recording's clock is
//! frozen, that is the clock's paused runtime, which the recording moves a
//! frame at a time (`snapshot::clock::tokio`). Elsewhere it is one runtime
//! on a thread of its own, on the real clock.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};

use iced::futures::future::BoxFuture;

use crate::snapshot::clock;

/// The runtime a tokio future made now runs in.
fn handle() -> tokio::runtime::Handle {
    clock::tokio().unwrap_or_else(|| background().clone())
}

/// One runtime on the real clock, on a thread that drives its timers.
fn background() -> &'static tokio::runtime::Handle {
    static BACKGROUND: OnceLock<tokio::runtime::Handle> = OnceLock::new();
    BACKGROUND.get_or_init(|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("a tokio runtime on the current thread builds");
        let handle = runtime.handle().clone();
        let _ = std::thread::Builder::new()
            .name("rawr tokio".into())
            .spawn(move || runtime.block_on(std::future::pending::<()>()));
        handle
    })
}

/// A future made and polled inside `handle`'s runtime.
struct Entered<F> {
    handle: tokio::runtime::Handle,
    future: Pin<Box<F>>,
}

impl<F: Future> Future for Entered<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<F::Output> {
        let _entered = self.handle.enter();
        self.future.as_mut().poll(context)
    }
}

/// The future `make` gives, made and polled inside tokio's runtime.
pub(crate) fn entered<F>(make: impl FnOnce() -> F) -> BoxFuture<'static, F::Output>
where
    F: Future + Send + 'static,
{
    let handle = handle();
    let future = {
        let _entered = handle.enter();
        Box::pin(make())
    };
    Box::pin(Entered { handle, future })
}

/// A timer's future, made and polled inside tokio's runtime. On a
/// recording's clock it keeps the clock running until it ends.
pub(crate) fn timer<F>(make: impl FnOnce() -> F) -> BoxFuture<'static, F::Output>
where
    F: Future + Send + 'static,
{
    let armed = clock::now().is_some().then(clock::Armed::new);
    let future = entered(make);
    Box::pin(async move {
        let output = future.await;
        drop(armed);
        output
    })
}

/// tokio's `time::sleep`.
pub(crate) fn sleep(duration: std::time::Duration) -> BoxFuture<'static, ()> {
    timer(move || tokio::time::sleep(duration))
}
