//! Final canonical internal-evidence capsule and payload digest.

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::metrics::recorder::Snapshot;

use super::{process, runner, Challenge, Error};

const CAPSULE_SCHEMA: &str = "rawr.phase9.evidence.capsule/v3";
const FAILURE_SCHEMA: &str = "rawr.phase9.evidence.startup-failure/v2";
const REASON_BYTES: usize = 16 * 1024;

/// Ordinary command outcome recorded only after production dispatch returns.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Product {
    Success,
    Failure,
}

/// Availability of the original product diagnostic, never a successful outcome.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Reason {
    /// Legacy callers supplied only `Product::Failure`.
    Unavailable,
    Complete {
        message: Box<str>,
    },
    /// The bounded prefix remains diagnostic evidence, with its reporting failure.
    Incomplete {
        message: Box<str>,
        fault: ReasonFault,
    },
}

/// Closed failures while retaining the original diagnostic.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReasonFault {
    Length,
    Formatting,
}

/// Failed startup evidence authenticated before any runner exists.
///
/// This data has no runner receipt or owner identity and can only return as an error.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    schema: Box<str>,
    pub challenge: Challenge,
    pub parent: process::Identity,
    pub cli: process::Identity,
    pub executable: process::file::Identity,
    pub counters: Snapshot,
    pub reason: Reason,
}

/// Digest-bound failure alternative on the existing authenticated startup frame.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Failed {
    payload: Failure,
    payload_digest: [u8; 32],
}

impl Failure {
    pub(super) fn transform(
        challenge: &Challenge,
        parent: process::Identity,
        cli: process::Identity,
        executable: process::file::Identity,
        counters: Snapshot,
        reason: Option<&dyn std::fmt::Display>,
    ) -> Result<Self, Error> {
        let failure = Self {
            schema: FAILURE_SCHEMA.into(),
            challenge: challenge.clone(),
            parent,
            cli,
            executable,
            counters,
            reason: Reason::capture(reason),
        };
        failure.validate(challenge, parent, cli)?;
        Ok(failure)
    }

    pub(super) fn validate(
        &self,
        challenge: &Challenge,
        parent: process::Identity,
        cli: process::Identity,
    ) -> Result<(), Error> {
        challenge.validate()?;
        validate_accounting(&self.counters)?;
        if self.schema.as_ref() != FAILURE_SCHEMA
            || &self.challenge != challenge
            || self.parent != parent
            || self.cli != cli
            || self.executable.digest != challenge.executable_digest
            || self.counters.runner.is_some()
            || self.counters.nodes.len() != 44
            || self.counters.processes.len() != 13
            || self.counters.milestones.len() != 6
            || !self.reason.valid()
        {
            return Err(Error::Binding("startup failure"));
        }
        Ok(())
    }

    /// Encode the full reached diagnostic under the existing evidence byte limit.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        super::canonical(self)
    }
}

impl Failed {
    pub(super) fn seal(payload: Failure) -> Result<Self, Error> {
        let payload_digest = failure_digest(&payload)?;
        Ok(Self {
            payload,
            payload_digest,
        })
    }

    pub(super) fn open(self) -> Result<Failure, Error> {
        if failure_digest(&self.payload)? != self.payload_digest {
            return Err(Error::Digest);
        }
        Ok(self.payload)
    }
}

impl Reason {
    fn capture(reason: Option<&dyn std::fmt::Display>) -> Self {
        let Some(reason) = reason else {
            return Self::Unavailable;
        };
        let mut buffer = ReasonBuffer {
            bytes: [0; REASON_BYTES],
            length: 0,
            overflow: false,
        };
        let result = std::fmt::write(&mut buffer, format_args!("{reason}"));
        // The buffer accepts only complete UTF-8 prefixes from fmt::Write::write_str.
        let message = match std::str::from_utf8(&buffer.bytes[..buffer.length]) {
            Ok(message) => Box::<str>::from(message),
            Err(_) => {
                return Self::Incomplete {
                    message: "".into(),
                    fault: ReasonFault::Formatting,
                }
            }
        };
        if buffer.overflow {
            Self::Incomplete {
                message,
                fault: ReasonFault::Length,
            }
        } else if result.is_err() || message.is_empty() {
            Self::Incomplete {
                message,
                fault: ReasonFault::Formatting,
            }
        } else {
            Self::Complete { message }
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::Unavailable => true,
            Self::Complete { message } => !message.is_empty() && message.len() <= REASON_BYTES,
            Self::Incomplete { message, .. } => message.len() <= REASON_BYTES,
        }
    }
}

struct ReasonBuffer {
    bytes: [u8; REASON_BYTES],
    length: usize,
    overflow: bool,
}

impl std::fmt::Write for ReasonBuffer {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let available = self.bytes.len() - self.length;
        let mut length = available.min(text.len());
        while !text.is_char_boundary(length) {
            length -= 1;
        }
        self.bytes[self.length..self.length + length].copy_from_slice(&text.as_bytes()[..length]);
        self.length += length;
        if length != text.len() {
            self.overflow = true;
            Err(std::fmt::Error)
        } else {
            Ok(())
        }
    }
}

fn failure_digest(payload: &Failure) -> Result<[u8; 32], Error> {
    let bytes = super::canonical(payload)?;
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.evidence.startup-failure-payload/v2");
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Ok(digest.finalize().into())
}

/// Typed availability of a current or future pipeline identity.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Identity {
    Measured { digest: [u8; 32] },
    NotApplicable { reason: Box<str> },
}

/// Complete internal facts for one authenticated CLI/runner launch.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Payload {
    schema: Box<str>,
    pub launch_id: [u8; 32],
    pub t0_ns: u64,
    pub parent: process::Identity,
    pub cli: process::Identity,
    pub executable: process::file::Identity,
    pub fixture_digest: [u8; 32],
    pub command_digest: [u8; 32],
    pub title: super::title::Identity,
    pub title_digest: [u8; 32],
    pub runner: runner::Receipt,
    pub counters: Snapshot,
    pub owner: Identity,
    pub artifact: Identity,
    pub overlay: Identity,
    pub product: Product,
    /// Original bounded diagnostic for a failed product, absent for success.
    pub failure_reason: Option<Reason>,
}

impl Payload {
    /// Encode reached successful/failed-runner facts under the existing byte bound.
    pub fn canonical_bytes(&self) -> Result<Box<[u8]>, Error> {
        super::canonical(self)
    }

    pub(super) fn transform(
        challenge: &Challenge,
        parent: process::Identity,
        cli: process::Identity,
        executable: process::file::Identity,
        runner: runner::Receipt,
        counters: Snapshot,
        product: Product,
    ) -> Result<Self, Error> {
        runner.validate(challenge, cli)?;
        if executable.digest != challenge.executable_digest {
            return Err(Error::Binding("capsule executable"));
        }
        validate_accounting(&counters)?;
        Ok(Self {
            schema: CAPSULE_SCHEMA.into(),
            launch_id: challenge.launch_id,
            t0_ns: challenge.t0_ns,
            parent,
            cli,
            executable,
            fixture_digest: challenge.fixture_digest,
            command_digest: challenge.command_digest,
            title: challenge.title.clone(),
            title_digest: challenge.title_digest,
            owner: Identity::Measured {
                digest: runner.generation_key,
            },
            artifact: Identity::Measured {
                digest: runner.artifact,
            },
            overlay: Identity::NotApplicable {
                reason: "the current linear owner has no separately published diagnostic overlay"
                    .into(),
            },
            runner,
            counters,
            product,
            failure_reason: None,
        })
    }

    pub(super) fn with_failure_reason(
        mut self,
        reason: &dyn std::fmt::Display,
    ) -> Result<Self, Error> {
        if self.product != Product::Failure {
            return Err(Error::Binding("successful capsule failure reason"));
        }
        self.failure_reason = Some(Reason::capture(Some(reason)));
        Ok(self)
    }

    pub(super) fn validate(
        &self,
        challenge: &Challenge,
        child: process::Identity,
        runner: &runner::Receipt,
    ) -> Result<(), Error> {
        validate_accounting(&self.counters)?;
        if self.schema.as_ref() != CAPSULE_SCHEMA
            || self
                .failure_reason
                .as_ref()
                .is_some_and(|reason| !reason.valid())
            || self.product == Product::Success && self.failure_reason.is_some()
            || self.launch_id != challenge.launch_id
            || self.t0_ns != challenge.t0_ns
            || self.cli != child
            || self.executable.digest != challenge.executable_digest
            || self.fixture_digest != challenge.fixture_digest
            || self.command_digest != challenge.command_digest
            || self.title != challenge.title
            || self.title_digest != challenge.title_digest
            || &self.runner != runner
            || self.owner
                != (Identity::Measured {
                    digest: runner.generation_key,
                })
            || self.artifact
                != (Identity::Measured {
                    digest: runner.artifact,
                })
            || self.counters.nodes.len() != 44
            || self.counters.processes.len() != 13
            || self.counters.milestones.len() != 6
        {
            return Err(Error::Binding("capsule"));
        }
        runner.validate(challenge, child)
    }
}

/// Payload plus an independently checked canonical digest.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Envelope {
    pub payload: Payload,
    pub payload_digest: [u8; 32],
}

impl Envelope {
    pub(super) fn seal(payload: Payload) -> Result<Self, Error> {
        let payload_digest = digest(&payload)?;
        Ok(Self {
            payload,
            payload_digest,
        })
    }

    pub(super) fn open(self) -> Result<Payload, Error> {
        if digest(&self.payload)? != self.payload_digest {
            return Err(Error::Digest);
        }
        Ok(self.payload)
    }
}

fn digest(payload: &Payload) -> Result<[u8; 32], Error> {
    let bytes = super::canonical(payload)?;
    let mut digest = Sha256::new();
    digest.update(b"rawr.phase9.evidence.capsule-payload/v3");
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
    Ok(digest.finalize().into())
}

fn validate_accounting(counters: &Snapshot) -> Result<(), Error> {
    if !counters.accounting.valid() {
        return Err(Error::Binding("capsule accounting"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn failure_reason_is_exact_or_explicitly_incomplete_with_a_utf8_safe_prefix() {
        assert_eq!(super::Reason::capture(None), super::Reason::Unavailable);
        assert_eq!(
            super::Reason::capture(Some(&"original")),
            super::Reason::Complete {
                message: "original".into()
            }
        );
        let large = "é".repeat(super::REASON_BYTES);
        match super::Reason::capture(Some(&large)) {
            super::Reason::Incomplete {
                message,
                fault: super::ReasonFault::Length,
            } => {
                assert_eq!(message.len(), super::REASON_BYTES);
                assert!(large.starts_with(message.as_ref()));
            }
            reason => panic!("expected explicit length failure, got {reason:?}"),
        }
        struct Broken;
        impl std::fmt::Display for Broken {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("retained prefix")?;
                Err(std::fmt::Error)
            }
        }
        assert_eq!(
            super::Reason::capture(Some(&Broken)),
            super::Reason::Incomplete {
                message: "retained prefix".into(),
                fault: super::ReasonFault::Formatting
            }
        );
    }

    #[test]
    fn capsule_keeps_recording_faults_as_diagnostics_but_rejects_malformed_rows() {
        let recorder = crate::metrics::recorder::Recorder::transform_at(1, 1).unwrap();
        let mut counters = recorder.snapshot().unwrap();
        counters.accounting.fault = Some(crate::metrics::accounting::Fault::GraphCapacity);
        assert!(super::validate_accounting(&counters).is_ok());
        let bytes = super::super::canonical(&counters).unwrap();
        let decoded: crate::metrics::recorder::Snapshot = super::super::decode(&bytes).unwrap();
        assert_eq!(decoded, counters);
        assert!(!decoded.accounting.qualifies(&[]));
        counters.accounting.scalars.swap(0, 1);
        assert!(super::validate_accounting(&counters).is_err());
    }
}
