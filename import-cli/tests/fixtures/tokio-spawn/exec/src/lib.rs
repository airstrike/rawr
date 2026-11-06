//! A trait whose method takes a future, as iced's `Executor::spawn` takes
//! `impl Future<Output = ()>`: the future is the core future, not a type
//! of the trait's own, so tokio's futures stay tokio's.

use std::future::Future;

/// What runs a future.
pub trait Executor {
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static);
}

/// An executor that drops what it is given.
pub struct Dropping;

impl Executor for Dropping {
    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        drop(future);
    }
}
