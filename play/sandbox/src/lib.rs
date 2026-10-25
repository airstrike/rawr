//! What a `?play` snippet reaches beyond iced, in the server and in the
//! browser alike.
//!
//! A snippet is no Cargo project, so the importer never runs for it (D513.5).
//! The crates a playground example names are given here instead, each as a
//! facade over the real crate:
//!
//! - [`rand`]: `rand::rng()` and its `random_range`, drawn by rand's own
//!   `StdRng`. The host seeds it. A server render takes a fixed seed, as it
//!   takes a frozen clock, so a snippet gives the same frames each time. The
//!   browser takes a seed from the page.
//! - [`tracing`]: `tracing_subscriber::fmt::init()` installs
//!   tracing-subscriber's own `fmt` subscriber, which writes where the host
//!   says.
//! - [`bundle`]: the files the playground's examples read with
//!   `include_bytes!`, shipped inside this crate and pinned by SHA-256.
//!   Only those paths resolve. Every other file stays unreadable.

pub mod bundle;
pub mod rand;
pub mod tracing;

use rawr::host;

/// The compile-time facades: register each on a snippet's Engine.
pub fn facades() -> Result<Vec<host::facade::Definition>, host::Error> {
    Ok(vec![rand::facade()?, tracing::facade()?])
}

/// The runtime facades that bind [`facades`], for an application's launch.
pub fn runtime_facades() -> Result<Vec<host::runtime::Facade>, host::Error> {
    Ok(vec![rand::runtime()?, tracing::runtime()?])
}

/// Register [`facades`] on `engine`, and the [`bundle`]'s files as the
/// module a snippet's `include_bytes!` reads them from.
pub fn install(engine: &mut rawr::Engine) -> Result<(), host::Error> {
    for facade in facades()? {
        engine.register_host_facade(facade)?;
    }
    bundle::install();
    engine.register_static_module(
        rawr::resolve::BUNDLE_MODULE,
        rawr::resolve::bundle_module().into(),
    );
    Ok(())
}
