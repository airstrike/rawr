//! `tracing_subscriber::fmt::init()` for a snippet: tracing-subscriber's own
//! `fmt` subscriber, as the global default, writing each line where the
//! host says ([`write_to`]).
//!
//! iced's examples call it first, so iced's events (and `log` records, which
//! it forwards) reach a terminal. A browser has its console, and rawr-web
//! writes there. A server render has no terminal: its stdout is the image,
//! so its lines go nowhere.
//!
//! Rust's `init` panics when a global subscriber is already set. A page runs
//! many programs in one process, and each calls `init` again, so a later call
//! keeps the subscriber the first one set, as `try_init` would.

use std::io;
use std::sync::OnceLock;

use rawr::host::{self, facade, item, signature};
use rawr::Dynamic;

static WRITER: OnceLock<fn(&str)> = OnceLock::new();

/// Write the subscriber's lines with `write`. Without it they go nowhere.
pub fn write_to(write: fn(&str)) {
    let _ = WRITER.set(write);
}

/// The `tracing_subscriber` facade.
pub fn facade() -> Result<facade::Definition, host::Error> {
    let mut builder = facade::Builder::new("tracing_subscriber", facade::Abi::new(16)?)?;
    builder.module("fmt")?;
    builder.item(item::Definition::function(
        "fmt::init",
        signature::Builder::new().finish(),
    )?)?;
    builder.finish()
}

/// The runtime bindings of [`facade`].
pub fn runtime() -> Result<host::runtime::Facade, host::Error> {
    let mut builder = host::runtime::Builder::new(facade()?);
    builder
        .bind("tracing_subscriber::fmt::init", |_| {
            let _ = tracing_subscriber::fmt()
                .with_ansi(false)
                // A browser has no system clock std can read.
                .without_time()
                .with_writer(|| Line(Vec::new()))
                .try_init();
            Ok(Dynamic::UNIT)
        })
        .expect("the tracing_subscriber facade declares fmt::init");
    Ok(builder.finish())
}

/// One event's text, written to the host's writer when it ends.
struct Line(Vec<u8>);

impl io::Write for Line {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for Line {
    fn drop(&mut self) {
        if let Some(write) = WRITER.get() {
            let text = String::from_utf8_lossy(&self.0);
            let text = text.trim_end();
            if !text.is_empty() {
                write(text);
            }
        }
    }
}
