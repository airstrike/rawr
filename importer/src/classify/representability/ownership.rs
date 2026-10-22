//! Exact receiver ownership admitted by Phase 2.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Receiver {
    Owned,
    Shared,
    Mutable,
}
