//! Exact dependency-graph watching and candidate compilation scheduling.

mod launch;
#[cfg(feature = "native-runner")]
pub mod native;
pub mod scheduler;
mod session;

pub(crate) use launch::{native_report, package_report, report, Launch};
pub(crate) use rawr_watch::Control;
pub use rawr_watch::{Adapter, Error, Notice};
pub(crate) use session::Session;
