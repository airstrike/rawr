//! Fixed scalar history for shared-store publication and retirement temporaries.
//!
//! Repeated targets accumulate counts in closed production families. This is not
//! a target journal and does not establish durable-mutation permission. Original
//! store, graph, pin and index accounts continue to govern every operation.

use serde::{Deserialize, Serialize};

use super::NodeKind;

const KINDS: [NodeKind; 11] = [
    NodeKind::Toolchain,
    NodeKind::Resolution,
    NodeKind::Api,
    NodeKind::Generic,
    NodeKind::Profile,
    NodeKind::Facade,
    NodeKind::Checked,
    NodeKind::ProfileProof,
    NodeKind::NativeProduct,
    NodeKind::Overlay,
    NodeKind::Runner,
];
const ROLES: [Role; 4] = [Role::Live, Role::LastGood, Role::Prepared, Role::InFlight];
const REPLACEMENTS: [Replacement; 13] = [
    Replacement::InstallationReservation,
    Replacement::InstallationBootstrap,
    Replacement::InstallationRepair,
    Replacement::InstallationSave,
    Replacement::InstallationPlanned,
    Replacement::InstallationMigration,
    Replacement::ProjectReservation,
    Replacement::ProjectCancel,
    Replacement::InFlightRelease,
    Replacement::ProjectRecovery,
    Replacement::ProjectRunnerRetirement,
    Replacement::PinBind,
    Replacement::PinRestore,
];
const RETIREMENTS: [Retirement; 7] = [
    Retirement::Enrollment,
    Retirement::Garbage,
    Retirement::Action,
    Retirement::Content,
    Retirement::Planned,
    Retirement::BundleRepair,
    Retirement::BundleEviction,
];

#[derive(Clone, Copy, Debug)]
pub enum Class {
    Action(NodeKind),
    Content(NodeKind),
    Index(NodeKind),
    Pin(Role),
    Replacement(Replacement),
    ProjectRetirement(Replacement),
    Retirement(Retirement),
    ReleasedPin(Role),
    StalePin(Role),
    RecoveredFile,
    RecoveredDirectory,
}

/// Source purpose, including the existing lock-backed transient-pin exception.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Replacement {
    InstallationReservation,
    InstallationBootstrap,
    InstallationRepair,
    InstallationSave,
    InstallationPlanned,
    InstallationMigration,
    ProjectReservation,
    ProjectCancel,
    InFlightRelease,
    ProjectRecovery,
    ProjectRunnerRetirement,
    PinBind,
    PinRestore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Retirement {
    Enrollment,
    Garbage,
    Action,
    Content,
    Planned,
    BundleRepair,
    BundleEviction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Live,
    LastGood,
    Prepared,
    InFlight,
}

/// Every increment observes an actual boundary, never an admitted reservation.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    Started,
    Created,
    Adopted,
    Renamed,
    Existing,
    Lost,
    OperationFailed,
    CleanupStarted,
    Cleaned,
    CleanupFailed,
    Returned,
    Unresolved,
    Anomaly,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub started: u64,
    pub created: u64,
    /// Pre-existing temporary adopted by actual bounded recovery, never a create.
    pub adopted: u64,
    /// Actual rename success, including a subsequent parent-sync failure.
    pub renamed: u64,
    /// Existing target observed before any temporary was created.
    pub existing: u64,
    /// Existing target observed after this call created its own temporary.
    pub lost: u64,
    pub operation_failed: u64,
    pub cleanup_started: u64,
    pub cleaned: u64,
    pub cleanup_failed: u64,
    pub returned: u64,
    pub unresolved: u64,
    pub anomaly: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub kind: NodeKind,
    pub action: Counts,
    pub content: Counts,
    pub index: Counts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub role: Role,
    pub counts: Counts,
    pub released: Counts,
    pub stale: Counts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReplacementRow {
    pub purpose: Replacement,
    pub counts: Counts,
    pub retired: Counts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementRow {
    pub purpose: Retirement,
    pub counts: Counts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault {
    Unscoped,
    Overflow,
    Shape,
}

/// Presence means the current observer was installed; historical absence is unknown.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub nodes: [Node; KINDS.len()],
    pub pins: [Pin; ROLES.len()],
    pub replacements: [ReplacementRow; REPLACEMENTS.len()],
    pub retirements: [RetirementRow; RETIREMENTS.len()],
    pub recovered_files: Counts,
    pub recovered_directories: Counts,
    pub fault: Option<Fault>,
}

impl Default for Evidence {
    fn default() -> Self {
        Self {
            nodes: KINDS.map(|kind| Node {
                kind,
                action: Counts::default(),
                content: Counts::default(),
                index: Counts::default(),
            }),
            pins: ROLES.map(|role| Pin {
                role,
                counts: Counts::default(),
                released: Counts::default(),
                stale: Counts::default(),
            }),
            replacements: REPLACEMENTS.map(|purpose| ReplacementRow {
                purpose,
                counts: Counts::default(),
                retired: Counts::default(),
            }),
            retirements: RETIREMENTS.map(|purpose| RetirementRow {
                purpose,
                counts: Counts::default(),
            }),
            recovered_files: Counts::default(),
            recovered_directories: Counts::default(),
            fault: None,
        }
    }
}

impl Evidence {
    pub(super) fn record(&mut self, class: Class, event: Event, limit: u64) {
        let counts = match class {
            Class::Action(kind) => self
                .nodes
                .iter_mut()
                .find(|row| row.kind == kind)
                .map(|row| &mut row.action),
            Class::Content(kind) => self
                .nodes
                .iter_mut()
                .find(|row| row.kind == kind)
                .map(|row| &mut row.content),
            Class::Index(kind) => self
                .nodes
                .iter_mut()
                .find(|row| row.kind == kind)
                .map(|row| &mut row.index),
            Class::Pin(role) => self
                .pins
                .iter_mut()
                .find(|row| row.role == role)
                .map(|row| &mut row.counts),
            Class::ReleasedPin(role) => self
                .pins
                .iter_mut()
                .find(|row| row.role == role)
                .map(|row| &mut row.released),
            Class::StalePin(role) => self
                .pins
                .iter_mut()
                .find(|row| row.role == role)
                .map(|row| &mut row.stale),
            Class::Replacement(purpose) => self
                .replacements
                .iter_mut()
                .find(|row| row.purpose == purpose)
                .map(|row| &mut row.counts),
            Class::ProjectRetirement(purpose) => self
                .replacements
                .iter_mut()
                .find(|row| row.purpose == purpose)
                .map(|row| &mut row.retired),
            Class::Retirement(purpose) => self
                .retirements
                .iter_mut()
                .find(|row| row.purpose == purpose)
                .map(|row| &mut row.counts),
            Class::RecoveredFile => Some(&mut self.recovered_files),
            Class::RecoveredDirectory => Some(&mut self.recovered_directories),
        };
        let Some(counts) = counts else {
            self.fault.get_or_insert(Fault::Shape);
            return;
        };
        let count = match event {
            Event::Started => &mut counts.started,
            Event::Created => &mut counts.created,
            Event::Adopted => &mut counts.adopted,
            Event::Renamed => &mut counts.renamed,
            Event::Existing => &mut counts.existing,
            Event::Lost => &mut counts.lost,
            Event::OperationFailed => &mut counts.operation_failed,
            Event::CleanupStarted => &mut counts.cleanup_started,
            Event::Cleaned => &mut counts.cleaned,
            Event::CleanupFailed => &mut counts.cleanup_failed,
            Event::Returned => &mut counts.returned,
            Event::Unresolved => &mut counts.unresolved,
            Event::Anomaly => &mut counts.anomaly,
        };
        match count.checked_add(1).filter(|next| *next <= limit) {
            Some(next) => *count = next,
            None => {
                self.fault.get_or_insert(Fault::Overflow);
            }
        }
    }

    /// Check complete successful temporary conservation in each closed family.
    /// This says nothing about the legality of target, ledger, or pin mutations.
    /// Recovered lifetimes end at removal, before the caller's batch parent sync;
    /// conservation alone does not establish durable completion of that batch.
    #[must_use]
    pub fn closed(&self) -> bool {
        self.fault.is_none()
            && self.nodes.iter().zip(KINDS).all(|(row, kind)| {
                row.kind == kind
                    && row.action.closed()
                    && row.content.closed()
                    && row.index.closed()
            })
            && self.pins.iter().zip(ROLES).all(|(row, role)| {
                row.role == role
                    && row.counts.closed()
                    && row.released.retired(false)
                    && row.stale.retired(false)
            })
            && self
                .replacements
                .iter()
                .zip(REPLACEMENTS)
                .all(|(row, purpose)| {
                    row.purpose == purpose && row.counts.closed() && row.retired.retired(false)
                })
            && self
                .retirements
                .iter()
                .zip(RETIREMENTS)
                .all(|(row, purpose)| row.purpose == purpose && row.counts.retired(false))
            && self.recovered_files.retired(true)
            && self.recovered_directories.retired(true)
    }

    /// The existing recorder bound also applies when inspecting decoded evidence.
    #[must_use]
    pub fn bounded(&self, limit: u64) -> bool {
        limit != 0
            && self.nodes.iter().all(|row| {
                [row.action, row.content, row.index]
                    .into_iter()
                    .all(|counts| counts.bounded(limit))
            })
            && self.pins.iter().all(|row| {
                [row.counts, row.released, row.stale]
                    .into_iter()
                    .all(|counts| counts.bounded(limit))
            })
            && self
                .replacements
                .iter()
                .all(|row| row.counts.bounded(limit) && row.retired.bounded(limit))
            && self.retirements.iter().all(|row| row.counts.bounded(limit))
            && self.recovered_files.bounded(limit)
            && self.recovered_directories.bounded(limit)
    }
}

impl Counts {
    fn bounded(self, limit: u64) -> bool {
        [
            self.started,
            self.created,
            self.adopted,
            self.renamed,
            self.existing,
            self.lost,
            self.operation_failed,
            self.cleanup_started,
            self.cleaned,
            self.cleanup_failed,
            self.returned,
            self.unresolved,
            self.anomaly,
        ]
        .into_iter()
        .all(|count| count <= limit)
    }

    #[must_use]
    pub fn closed(self) -> bool {
        self.adopted == 0
            && self.operation_failed == 0
            && self.cleanup_failed == 0
            && self.unresolved == 0
            && self.anomaly == 0
            && self.returned == self.started
            && self.created.checked_add(self.existing) == Some(self.started)
            && self.renamed.checked_add(self.lost) == Some(self.created)
            && self.cleaned == self.lost
            && self.cleanup_started == self.cleaned
    }

    #[must_use]
    pub fn retired(self, adopted: bool) -> bool {
        self.operation_failed == 0
            && self.cleanup_failed == 0
            && self.unresolved == 0
            && self.anomaly == 0
            && self.returned == self.started
            && self.renamed == 0
            && self.existing == 0
            && self.lost == 0
            && (if adopted {
                self.created == 0 && self.adopted == self.started
            } else {
                self.adopted == 0 && self.created == self.started
            })
            && self.cleaned == self.started
            && self.cleanup_started == self.cleaned
    }

    /// Exact cold publication count owned by this process's node executions.
    #[must_use]
    pub fn winners(self, count: u64) -> bool {
        self == Self {
            started: count,
            created: count,
            renamed: count,
            returned: count,
            ..Self::default()
        }
    }
}

/// Local observation of one existing call. It owns no filesystem or cleanup power.
/// Drop emits reached failures even when the original function returns early.
pub(crate) struct Attempt {
    class: Class,
    created: bool,
    adopted: bool,
    renamed: bool,
    cleaned: bool,
    operation: bool,
}

impl Attempt {
    pub(crate) fn start(class: Class) -> Self {
        super::record_publication(class, Event::Started);
        Self {
            class,
            created: false,
            adopted: false,
            renamed: false,
            cleaned: false,
            operation: false,
        }
    }

    pub(crate) fn created(&mut self) {
        if self.created {
            self.emit(Event::Anomaly);
        }
        self.created = true;
        self.emit(Event::Created);
    }

    pub(crate) fn adopted(&mut self) {
        if self.created || self.adopted {
            self.emit(Event::Anomaly);
        }
        self.adopted = true;
        self.emit(Event::Adopted);
    }

    pub(crate) fn renamed(&mut self) {
        if !self.created || self.renamed {
            self.emit(Event::Anomaly);
        }
        self.renamed = true;
        self.emit(Event::Renamed);
    }

    /// None means the existing publication body failed; Some records Existing.
    pub(crate) fn operation(&mut self, existing: Option<bool>) {
        if self.operation {
            self.emit(Event::Anomaly);
        }
        self.operation = true;
        match existing {
            None => self.emit(Event::OperationFailed),
            Some(true) => self.emit(if self.created {
                Event::Lost
            } else {
                Event::Existing
            }),
            Some(false) => {}
        }
    }

    pub(crate) fn cleanup_started(&self) {
        if (!self.created && !self.adopted) || self.renamed || self.cleaned {
            self.emit(Event::Anomaly);
        }
        self.emit(Event::CleanupStarted);
    }

    pub(crate) fn cleanup(&mut self, succeeded: bool) {
        if succeeded {
            self.cleaned = true;
            self.emit(Event::Cleaned);
        } else {
            self.emit(Event::CleanupFailed);
        }
    }

    fn emit(&self, event: Event) {
        super::record_publication(self.class, event);
    }

    pub(crate) fn finish<T, E>(mut self, result: Result<T, E>) -> Result<T, E> {
        self.operation(result.as_ref().ok().map(|_| false));
        result
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        if !self.operation {
            self.emit(Event::OperationFailed);
        }
        if (self.created || self.adopted) && !self.renamed && !self.cleaned {
            self.emit(Event::Unresolved);
        }
        self.emit(Event::Returned);
    }
}

#[cfg(test)]
mod tests {
    use super::{Class, Counts, Event, Evidence, Fault};
    use crate::metrics::NodeKind;

    #[test]
    fn publication_history_accumulates_repeated_lifetimes_without_target_storage() {
        let mut evidence = Evidence::default();
        for events in [
            &[
                Event::Started,
                Event::Created,
                Event::Renamed,
                Event::Returned,
            ][..],
            &[
                Event::Started,
                Event::Created,
                Event::Lost,
                Event::CleanupStarted,
                Event::Cleaned,
                Event::Returned,
            ],
            &[Event::Started, Event::Existing, Event::Returned],
        ] {
            for event in events {
                evidence.record(Class::Content(NodeKind::Toolchain), *event, 10);
            }
        }
        assert!(evidence.closed());
        assert!(evidence.bounded(10));
        assert!(!evidence.bounded(2));
        let counts = evidence.nodes[0].content;
        assert_eq!(
            (
                counts.started,
                counts.created,
                counts.renamed,
                counts.lost,
                counts.existing
            ),
            (3, 2, 1, 1, 1)
        );
        assert!(!counts.winners(1));
        evidence.record(Class::Content(NodeKind::Toolchain), Event::Started, 10);
        assert!(
            !evidence.closed(),
            "an unmatched lifetime remains incomplete"
        );
    }

    #[test]
    fn publication_history_refuses_overflow_bad_shape_and_failed_cleanup() {
        let mut evidence = Evidence::default();
        for _ in 0..2 {
            evidence.record(Class::Action(NodeKind::Runner), Event::Started, 1);
        }
        assert_eq!(evidence.fault, Some(Fault::Overflow));
        assert!(!evidence.closed());
        let mut evidence = Evidence::default();
        evidence.nodes.swap(0, 1);
        assert!(!evidence.closed());
        assert!(!Counts {
            started: u64::MAX,
            returned: u64::MAX,
            created: u64::MAX,
            existing: 1,
            ..Counts::default()
        }
        .closed());
        assert!(!Counts {
            cleanup_failed: 1,
            ..Counts::default()
        }
        .closed());
    }
}
