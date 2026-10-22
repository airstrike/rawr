//! Canonical child-process-counter evidence.

use crate::metrics::ProcessKind;

/// One fixed child-process counter row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Count {
    pub kind: ProcessKind,
    pub count: u64,
}
