//! Closed process and executable identities for fresh-process evidence.

use serde::{Deserialize, Serialize};

pub mod file;

/// Exact kernel process identity facts carried by the authenticated channel.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Identity {
    pub pid: u32,
    pub uid: u32,
}

impl Identity {
    pub(super) const fn transform(pid: u32, uid: u32) -> Self {
        Self { pid, uid }
    }

    pub(super) fn current() -> Self {
        Self {
            pid: std::process::id(),
            uid: nix::unistd::Uid::effective().as_raw(),
        }
    }
}
