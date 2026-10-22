//! Canonical lifecycle-milestone evidence.

use crate::metrics::Milestone;

/// One fixed lifecycle observation row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Count {
    pub milestone: Milestone,
    pub count: u64,
    pub last_observed_ns: u64,
    pub last_elapsed_ns: u64,
}
