//! Canonical node-counter evidence.

use crate::metrics::{NodeEvent, NodeKind};

/// One fixed node/event counter row.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Count {
    pub kind: NodeKind,
    pub event: NodeEvent,
    pub count: u64,
}
