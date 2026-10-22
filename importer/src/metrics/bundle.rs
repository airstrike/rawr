//! Fixed observations of actual bundle attempt lifetimes, never storage authority.
//!
//! The two slots are the production resolution and runner roles, not a journal
//! capacity. A second identity makes the narrow observation ambiguous. Original
//! graph, store and recovery accounts continue to govern every operation.

use serde::{Deserialize, Serialize};

use super::{pipeline::target::Target, NodeKind};

/// Existing production boundaries; requests or reservations are not allocations.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Allocated,
    Initialized,
    InitializationFailed,
    SealStarted,
    Sealed,
    PublicationStarted,
    Published,
    Retained,
    CleanupStarted,
    ThawFailed,
    Removed,
    RemovalFailed,
}

/// Checked scalar counts associated with one actual bundle identity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub target: Option<Target>,
    pub allocated: u64,
    pub initialized: u64,
    pub initialization_failed: u64,
    pub seal_started: u64,
    pub sealed: u64,
    pub publication_started: u64,
    pub published: u64,
    pub retained: u64,
    pub cleanup_started: u64,
    pub thaw_failed: u64,
    pub removed: u64,
    pub removal_failed: u64,
}

/// First loss of the fixed history's representability; later counts remain useful.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault {
    Unscoped,
    Kind,
    Identity,
    Overflow,
}

/// Presence proves the current scoped observer was installed; absence is unknown.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub resolution: Row,
    pub runner: Row,
    pub fault: Option<Fault>,
}

impl Evidence {
    pub(super) fn record(&mut self, kind: NodeKind, target: Target, event: Event, limit: u64) {
        let row = match kind {
            NodeKind::Resolution => &mut self.resolution,
            NodeKind::Runner => &mut self.runner,
            _ => {
                self.fault.get_or_insert(Fault::Kind);
                return;
            }
        };
        if !target.valid() || row.target.is_some_and(|previous| previous != target) {
            self.fault.get_or_insert(Fault::Identity);
        }
        row.target.get_or_insert(target);
        let counter = match event {
            Event::Allocated => &mut row.allocated,
            Event::Initialized => &mut row.initialized,
            Event::InitializationFailed => &mut row.initialization_failed,
            Event::SealStarted => &mut row.seal_started,
            Event::Sealed => &mut row.sealed,
            Event::PublicationStarted => &mut row.publication_started,
            Event::Published => &mut row.published,
            Event::Retained => &mut row.retained,
            Event::CleanupStarted => &mut row.cleanup_started,
            Event::ThawFailed => &mut row.thaw_failed,
            Event::Removed => &mut row.removed,
            Event::RemovalFailed => &mut row.removal_failed,
        };
        match counter.checked_add(1).filter(|value| *value <= limit) {
            Some(value) => *counter = value,
            None => {
                self.fault.get_or_insert(Fault::Overflow);
            }
        }
    }
}

impl Row {
    /// Exact successful winner history for the target already proved by the owner.
    /// This does not cover lightweight publication temporaries or general writes.
    #[must_use]
    pub fn retained_once(self, target: Target) -> bool {
        target.valid()
            && self
                == Self {
                    target: Some(target),
                    allocated: 1,
                    initialized: 1,
                    seal_started: 1,
                    sealed: 1,
                    publication_started: 1,
                    published: 1,
                    retained: 1,
                    ..Self::default()
                }
    }

    /// Explicitly observed absence of any attempt in this bundle-owning role.
    #[must_use]
    pub fn untouched(self) -> bool {
        self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{Event, Evidence, Fault};
    use crate::metrics::{pipeline::target::Target, NodeKind};

    fn target(value: u8) -> Target {
        Target::observe(crate::store::bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes([1; 32]),
            crate::store::bundle::BundleId::from_bytes([value; 32]),
        ))
    }

    #[test]
    fn bundle_history_checks_exact_retention_and_keeps_cleanup_failures() {
        let mut history = Evidence::default();
        for event in [
            Event::Allocated,
            Event::Initialized,
            Event::SealStarted,
            Event::Sealed,
            Event::PublicationStarted,
            Event::Published,
            Event::Retained,
        ] {
            history.record(NodeKind::Resolution, target(2), event, 10);
        }
        assert!(history.resolution.retained_once(target(2)));
        assert!(history.runner.untouched());
        assert!(!history.resolution.retained_once(target(3)));
        for event in [
            Event::CleanupStarted,
            Event::ThawFailed,
            Event::RemovalFailed,
        ] {
            history.record(NodeKind::Resolution, target(2), event, 10);
        }
        assert!(!history.resolution.retained_once(target(2)));
        assert_eq!(history.resolution.thaw_failed, 1);
        assert_eq!(history.resolution.removal_failed, 1);
    }

    #[test]
    fn bundle_history_bounds_each_count_and_never_replaces_first_identity_or_fault() {
        let mut history = Evidence::default();
        history.record(NodeKind::Runner, target(2), Event::Allocated, 1);
        history.record(NodeKind::Runner, target(3), Event::Allocated, 1);
        history.record(NodeKind::Runner, target(3), Event::CleanupStarted, 1);
        assert_eq!(history.fault, Some(Fault::Identity));
        assert_eq!(history.runner.target, Some(target(2)));
        assert_eq!(history.runner.allocated, 1);
        assert_eq!(history.runner.cleanup_started, 1);
        let mut overflow = Evidence::default();
        overflow.record(NodeKind::Runner, target(2), Event::Allocated, 1);
        overflow.record(NodeKind::Runner, target(2), Event::Allocated, 1);
        assert_eq!(overflow.fault, Some(Fault::Overflow));
        let mut unknown = Evidence::default();
        unknown.record(NodeKind::Api, target(2), Event::Allocated, 1);
        assert_eq!(unknown.fault, Some(Fault::Kind));
    }
}
