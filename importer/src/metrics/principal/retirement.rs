//! Actual non-pin retirement and removal occurrences, not deletion authority.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{Account, Commitment, Fault, Lease, Observation};
use crate::node::account::evidence::Usage;
use crate::store::Publication;

pub mod join;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Target {
    Enrollment {
        project: [u8; 32],
    },
    Action {
        kind: u8,
        storage: u16,
        action: [u8; 32],
    },
    Content {
        kind: u8,
        schema: u32,
        content: [u8; 32],
    },
    Bundle {
        execution: [u8; 32],
        bundle: [u8; 32],
    },
    IndexBucket {
        kind: u8,
        lookup: [u8; 32],
    },
    IndexRow {
        kind: u8,
        lookup: [u8; 32],
        action: [u8; 32],
    },
}

impl Target {
    pub fn relative(self) -> Option<PathBuf> {
        let digest = |bytes| crate::node::Digest::from_bytes(bytes).directory();
        Some(match self {
            Self::Enrollment { project } => {
                PathBuf::from("locks/projects").join(format!("{}.pending", digest(project)))
            }
            Self::Action {
                kind,
                storage,
                action,
            } => PathBuf::from("actions")
                .join(crate::node::Kind::from_code(kind)?.directory())
                .join(format!("{storage:04x}"))
                .join(digest(action)),
            Self::Content {
                kind,
                schema,
                content,
            } => crate::store::content_relative(
                crate::node::Kind::from_code(kind)?,
                schema,
                crate::node::Digest::from_bytes(content),
            ),
            Self::Bundle { execution, bundle } => PathBuf::from("executions")
                .join(digest(execution))
                .join(digest(bundle)),
            Self::IndexBucket { kind, lookup } => PathBuf::from("indexes")
                .join(crate::node::Kind::from_code(kind)?.directory())
                .join(digest(lookup)),
            Self::IndexRow {
                kind,
                lookup,
                action,
            } => crate::store::index_relative(
                crate::node::Kind::from_code(kind)?,
                crate::node::LookupKey::from_bytes(lookup),
                crate::node::ActionKey::from_bytes(action),
            ),
        })
    }

    /// Parse only the producer's existing fixed namespace, without path copies.
    pub(crate) fn parse(path: &Path) -> Option<Self> {
        let mut parts = path
            .as_os_str()
            .as_encoded_bytes()
            .split(|byte| *byte == b'/');
        let first = parts.next()?;
        let second = parts.next()?;
        let third = parts.next()?;
        let fourth = parts.next();
        if parts.next().is_some() {
            return None;
        }
        match (first, second, third, fourth) {
            (b"locks", b"projects", name, None) => Some(Self::Enrollment {
                project: digest(name.strip_suffix(b".pending")?)?,
            }),
            (b"executions", execution, bundle, None) => Some(Self::Bundle {
                execution: digest(execution)?,
                bundle: digest(bundle)?,
            }),
            (b"actions", name, storage, Some(action)) if storage.len() == 4 => Some(Self::Action {
                kind: kind(name)?,
                storage: hexadecimal(storage)? as u16,
                action: digest(action)?,
            }),
            (b"content", name, schema, Some(content)) if schema.len() == 8 => Some(Self::Content {
                kind: kind(name)?,
                schema: hexadecimal(schema)?,
                content: digest(content)?,
            }),
            (b"indexes", name, lookup, None) => Some(Self::IndexBucket {
                kind: kind(name)?,
                lookup: digest(lookup)?,
            }),
            (b"indexes", name, lookup, Some(action)) => Some(Self::IndexRow {
                kind: kind(name)?,
                lookup: digest(lookup)?,
                action: digest(action)?,
            }),
            _ => None,
        }
    }
}

pub(super) fn kind(name: &[u8]) -> Option<u8> {
    (1..=11).find(|code| {
        crate::node::Kind::from_code(*code).is_some_and(|kind| kind.directory().as_bytes() == name)
    })
}
pub(super) fn hexadecimal(bytes: &[u8]) -> Option<u32> {
    bytes.iter().try_fold(0_u32, |value, byte| {
        value.checked_mul(16)?.checked_add(match byte {
            b'0'..=b'9' => (byte - b'0') as u32,
            b'a'..=b'f' => (byte - b'a' + 10) as u32,
            _ => return None,
        })
    })
}
pub(super) fn digest(bytes: &[u8]) -> Option<[u8; 32]> {
    if bytes.len() != 64 {
        return None;
    }
    let mut result = [0; 32];
    for (value, pair) in result.iter_mut().zip(bytes.chunks_exact(2)) {
        *value = hexadecimal(pair)? as u8;
    }
    Some(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    Enrollment,
    Garbage,
    ActionEviction,
    ContentEviction,
    Planned { lease: Lease },
    BundleRepair,
    BundleEviction,
    IndexRepair,
    IndexActionRetirement,
}

/// Already validated allocation/record facts. A record's payload commitment is
/// supplied only when this caller actually validated the payload bytes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub allocation: Option<super::pin::Reference>,
    pub owner: Option<Owner>,
    pub member: Option<Commitment>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Owner {
    pub lease: Lease,
    pub publisher: [u8; 32],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Temporary {
    pub process: u32,
    pub sequence: u64,
}
impl Temporary {
    pub fn relative(self) -> PathBuf {
        PathBuf::from("temporaries").join(format!("{}-{}", self.process, self.sequence))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "retirement", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Event {
    Started {
        attempt: u64,
        publication: u64,
        root: [u8; 32],
        relative: [u8; 32],
        target: Target,
        cause: Cause,
        facts: Facts,
    },
    Relocated {
        attempt: u64,
        temporary: Temporary,
        relative: [u8; 32],
    },
    RemovalStarted {
        attempt: u64,
    },
    RemovalFinished {
        attempt: u64,
        succeeded: bool,
    },
    Finished {
        attempt: u64,
        succeeded: bool,
    },
}

pub(crate) struct Attempt {
    token: u64,
    usage: Option<Usage>,
    succeeded: Cell<bool>,
}
impl Attempt {
    pub(crate) fn start(
        publication: &Publication<'_>,
        path: &Path,
        cause: Cause,
        facts: Facts,
    ) -> Self {
        let mut value = Self {
            token: 0,
            usage: None,
            succeeded: Cell::new(false),
        };
        if !crate::metrics::principal_enabled() {
            return value;
        }
        value.token = super::next();
        value.usage = Some(Usage::retain(publication.graph()));
        let event = (|| {
            if value.token == 0 {
                return Err(Fault::Exhausted);
            }
            let relative = path
                .strip_prefix(publication.root())
                .map_err(|_| Fault::Identity)?;
            for size in [
                publication.root().as_os_str().as_encoded_bytes().len(),
                relative.as_os_str().as_encoded_bytes().len(),
            ] {
                publication.graph().hash(size).map_err(|_| Fault::Budget)?;
            }
            let target = Target::parse(relative).ok_or(Fault::Identity)?;
            if !compatible(target, cause, facts) {
                return Err(Fault::Identity);
            }
            Ok(Event::Started {
                attempt: value.token,
                publication: publication.observation(),
                root: super::path_digest(publication.root()),
                relative: super::path_digest(relative),
                target,
                cause,
                facts,
            })
        })();
        value.emit(event);
        value
    }
    pub(crate) fn relocated(
        &self,
        publication: &Publication<'_>,
        path: &Path,
        process: u32,
        sequence: u64,
    ) {
        if self.usage.is_none() || !crate::metrics::principal_enabled() {
            return;
        }
        let event = (|| {
            let relative = path
                .strip_prefix(publication.root())
                .map_err(|_| Fault::Identity)?;
            publication
                .graph()
                .hash(relative.as_os_str().as_encoded_bytes().len())
                .map_err(|_| Fault::Budget)?;
            Ok(Event::Relocated {
                attempt: self.token,
                temporary: Temporary { process, sequence },
                relative: super::path_digest(relative),
            })
        })();
        self.emit(event);
    }
    pub(crate) fn removal_started(&self) {
        self.emit(Ok(Event::RemovalStarted {
            attempt: self.token,
        }));
    }
    pub(crate) fn removal_finished(&self, succeeded: bool) {
        self.emit(Ok(Event::RemovalFinished {
            attempt: self.token,
            succeeded,
        }));
    }
    pub(crate) fn completed(&self, succeeded: bool) {
        self.succeeded.set(succeeded);
    }
    fn emit(&self, event: Result<Event, Fault>) {
        let Some(usage) = &self.usage else {
            return;
        };
        if crate::metrics::principal_enabled() {
            crate::metrics::record_principal(&Observation {
                event: event.map(|change| super::Event::Retirement { change }),
                budget: Account::Retained(usage),
            });
        }
    }
}
impl Drop for Attempt {
    fn drop(&mut self) {
        self.emit(Ok(Event::Finished {
            attempt: self.token,
            succeeded: self.succeeded.get(),
        }));
    }
}

pub(super) fn compatible(target: Target, cause: Cause, facts: Facts) -> bool {
    let shape = match (target, cause) {
        (Target::Enrollment { .. }, Cause::Enrollment)
        | (Target::Action { .. } | Target::Content { .. }, Cause::Garbage)
        | (Target::Action { storage: 2, .. }, Cause::ActionEviction | Cause::Planned { .. })
        | (Target::Content { .. }, Cause::ContentEviction | Cause::Planned { .. })
        | (Target::Bundle { .. }, Cause::BundleRepair | Cause::BundleEviction)
        | (Target::IndexBucket { .. }, Cause::IndexRepair)
        | (Target::IndexRow { .. }, Cause::IndexActionRetirement) => true,
        _ => false,
    };
    if !shape {
        return false;
    }
    let required = matches!(
        cause,
        Cause::ActionEviction
            | Cause::ContentEviction
            | Cause::Planned { .. }
            | Cause::BundleRepair
            | Cause::BundleEviction
    );
    if required != facts.allocation.is_some() {
        return false;
    }
    if let Some(allocation) = facts.allocation {
        if allocation.allocations != 1
            || allocation.members == 0
            || allocation.record == [0; 32]
            || crate::node::Kind::from_code(allocation.kind).is_none()
        {
            return false;
        }
        let matches = match target {
            Target::Action { kind, action, .. } => {
                allocation.class == 1 && allocation.kind == kind && allocation.key == action
            }
            Target::Content {
                kind,
                schema,
                content,
            } => {
                allocation.class == 2
                    && allocation.kind == kind
                    && allocation.schema == schema
                    && allocation.key == content
            }
            Target::Bundle { .. } => allocation.class == 3,
            _ => false,
        };
        if !matches {
            return false;
        }
    }
    let record = matches!(
        cause,
        Cause::ActionEviction | Cause::ContentEviction | Cause::Planned { .. }
    );
    if record != facts.owner.is_some() {
        return false;
    }
    let payload = matches!(cause, Cause::ActionEviction | Cause::Planned { .. });
    if payload != facts.member.is_some() {
        return false;
    }
    if let Cause::Planned { lease } = cause {
        if facts.owner.is_none_or(|owner| owner.lease != lease) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{Cause, Facts, Target};

    #[test]
    fn legacy_garbage_targets_preserve_the_complete_control_grammar() {
        for code in 1..=11 {
            let kind = crate::node::Kind::from_code(code).unwrap();
            for storage in [0, 1, 2, u16::MAX] {
                let path = std::path::PathBuf::from(format!(
                    "actions/{}/{storage:04x}/{}",
                    kind.directory(),
                    "00".repeat(32)
                ));
                let target = Target::parse(&path).unwrap();
                assert_eq!(target.relative().as_deref(), Some(path.as_path()));
                assert!(super::compatible(target, Cause::Garbage, Facts::default()));
            }
            for schema in [0, 1, u32::MAX] {
                let path = std::path::PathBuf::from(format!(
                    "content/{}/{schema:08x}/{}",
                    kind.directory(),
                    "ff".repeat(32)
                ));
                let target = Target::parse(&path).unwrap();
                assert_eq!(target.relative().as_deref(), Some(path.as_path()));
                assert!(super::compatible(target, Cause::Garbage, Facts::default()));
            }
        }
        for path in [
            "actions/unknown/0001",
            "actions/runner/00001",
            "content/runner/0000000G",
        ] {
            assert!(Target::parse(&std::path::Path::new(path).join("ab".repeat(32))).is_none());
        }
    }
}
