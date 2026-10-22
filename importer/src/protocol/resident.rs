//! Fixed-width resident request/evidence metadata, not live-owner authority.
//!
//! Only the authenticated admitted process may produce the actual candidate.
//! These codecs bind its response to the original HotBegin and exact snapshot;
//! the process owner and child publication gate still authorize installation.

use sha2::{Digest as _, Sha256};

const VERSION: u32 = 1;
const HEADER: usize = 100;
const EVIDENCE: usize = HEADER + 32 + 32 + 1 + 32;

/// One original attempt and snapshot, without parent-derived application uses.
pub struct Attempt {
    header: [u8; HEADER],
}

/// Evidence from the actual compiled candidate, before live-owner admission.
pub struct Compiled {
    bytes: [u8; EVIDENCE],
}

/// Malformed or unrelated resident evidence must never request a Cargo retry.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("resident request or response has invalid framing")]
    Framing,
    #[error("resident request version is unsupported")]
    Version,
    #[error("resident request is unrelated to the original hot attempt")]
    Attempt,
    #[error("resident snapshot bytes do not match their request")]
    Snapshot,
    #[error("resident compiler carrier does not match the admitted owner")]
    Carrier,
    #[error("resident transport allocation failed")]
    Allocation,
}

impl Attempt {
    /// Correlation carried by every no-install outcome, without renewing it.
    #[must_use]
    pub fn response_header(&self) -> &[u8] {
        &self.header
    }

    /// Authenticate a terminal no-install reply against this exact request.
    pub fn accept_status<'a>(&self, bytes: &'a [u8]) -> Result<&'a str, Error> {
        if bytes.len() > super::FRAME_BYTES - 5 {
            return Err(Error::Framing);
        }
        let (header, diagnostic) = bytes.split_first_chunk::<HEADER>().ok_or(Error::Framing)?;
        if *header != self.header {
            return Err(Error::Attempt);
        }
        std::str::from_utf8(diagnostic).map_err(|_| Error::Framing)
    }

    pub(crate) fn bind(
        identity: [u8; 56],
        revision: crate::supervisor::Revision,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Self {
        Self::header(identity, revision.get(), snapshot.digest())
    }

    fn header(identity: [u8; 56], revision: u64, snapshot: &[u8; 32]) -> Self {
        let mut header = [0; HEADER];
        header[..4].copy_from_slice(&VERSION.to_le_bytes());
        header[4..60].copy_from_slice(&identity);
        header[60..68].copy_from_slice(&revision.to_le_bytes());
        header[68..].copy_from_slice(snapshot);
        Self { header }
    }

    pub(crate) fn encode(
        &self,
        snapshot: crate::supervisor::snapshot::Bytes,
    ) -> Result<Vec<u8>, Error> {
        if self.header[68..] != *snapshot.digest() {
            return Err(Error::Snapshot);
        }
        let snapshot = snapshot.into_inner();
        let length = HEADER.checked_add(snapshot.len()).ok_or(Error::Framing)?;
        if length > super::FRAME_BYTES - 5 {
            return Err(Error::Framing);
        }
        let mut payload = Vec::new();
        payload
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        payload.extend_from_slice(&self.header);
        payload.extend_from_slice(&snapshot);
        Ok(payload)
    }

    /// Decode against the exact original HotBegin retained by the child. The
    /// returned snapshot still requires the normal bounded package decoder.
    pub fn decode<'a>(payload: &'a [u8], original: &[u8; 56]) -> Result<(Self, &'a [u8]), Error> {
        if payload.len() > super::FRAME_BYTES - 5 {
            return Err(Error::Framing);
        }
        let (header, snapshot) = payload
            .split_first_chunk::<HEADER>()
            .ok_or(Error::Framing)?;
        if header[..4] != VERSION.to_le_bytes() {
            return Err(Error::Version);
        }
        if header[4..60] != *original || header[60..68] == [0; 8] || original[48..] == [0; 8] {
            return Err(Error::Attempt);
        }
        if snapshot.is_empty() || header[68..] != <[u8; 32]>::from(Sha256::digest(snapshot)) {
            return Err(Error::Snapshot);
        }
        Ok((Self { header: *header }, snapshot))
    }

    /// Join the actual child compiler output to this request. The caller must
    /// retain that same candidate through the normal commit/abort transaction.
    pub fn compiled(
        self,
        carrier: [u8; 32],
        coverage: [u8; 32],
        semantic: Option<[u8; 32]>,
    ) -> Compiled {
        let mut bytes = [0; EVIDENCE];
        bytes[..HEADER].copy_from_slice(&self.header);
        bytes[HEADER..HEADER + 32].copy_from_slice(&carrier);
        bytes[HEADER + 32..HEADER + 64].copy_from_slice(&coverage);
        if let Some(semantic) = semantic {
            bytes[HEADER + 64] = 1;
            bytes[HEADER + 65..].copy_from_slice(&semantic);
        }
        Compiled { bytes }
    }

    /// Accept only evidence for this request from its admitted carrier. This
    /// check requires the authenticated live channel; bytes alone are no proof.
    pub fn accept(&self, bytes: &[u8], carrier: &[u8; 32]) -> Result<Compiled, Error> {
        let bytes: &[u8; EVIDENCE] = bytes.try_into().map_err(|_| Error::Framing)?;
        if bytes[..HEADER] != self.header {
            return Err(Error::Attempt);
        }
        if bytes[HEADER..HEADER + 32] != *carrier {
            return Err(Error::Carrier);
        }
        match bytes[HEADER + 64] {
            0 if bytes[HEADER + 65..] == [0; 32] => {}
            1 => {}
            _ => return Err(Error::Framing),
        }
        Ok(Compiled { bytes: *bytes })
    }
}

impl Compiled {
    /// Advance the existing profile route from its admitted receipt.
    #[must_use]
    pub fn advance_receipt(&self, previous: [u8; 32]) -> [u8; 32] {
        crate::supervisor::activation::advance(previous, &self.key())
    }

    /// Fixed bounded reply bytes; no source text or runtime values are encoded.
    #[must_use]
    pub fn encode(&self) -> &[u8] {
        &self.bytes
    }

    /// Distinct generation domain bound to predecessor, nonce, sequence,
    /// revision, snapshot, carrier and the actual compilation's evidence.
    /// The predecessor chain retains the admitted epoch/artifact identity.
    #[must_use]
    pub fn key(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.resident-generation/v1");
        digest.update(self.bytes);
        digest.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt() -> Attempt {
        let mut identity = [7; 56];
        identity[48..].copy_from_slice(&1_u64.to_le_bytes());
        Attempt::header(identity, 2, &Sha256::digest(b"snapshot").into())
    }

    #[test]
    fn original_attempt_snapshot_and_carrier_are_all_required() {
        let attempt = attempt();
        let mut payload = attempt.header.to_vec();
        payload.extend_from_slice(b"snapshot");
        let original = attempt.header[4..60].try_into().unwrap();
        let (decoded, snapshot) = Attempt::decode(&payload, original).unwrap();
        assert_eq!(snapshot, b"snapshot");
        let evidence = decoded.compiled([1; 32], [2; 32], Some([3; 32]));
        let accepted = attempt.accept(evidence.encode(), &[1; 32]).unwrap();
        assert_eq!(accepted.key(), evidence.key());
        assert!(matches!(
            attempt.accept(evidence.encode(), &[4; 32]),
            Err(Error::Carrier)
        ));
        for field in [4, 36, 52, 60, 68] {
            let mut changed = evidence.bytes;
            changed[field] ^= 1;
            assert!(matches!(
                attempt.accept(&changed, &[1; 32]),
                Err(Error::Attempt)
            ));
        }
        payload[HEADER] ^= 1;
        assert!(matches!(
            Attempt::decode(&payload, original),
            Err(Error::Snapshot)
        ));
    }

    #[test]
    fn every_identity_input_changes_the_generation_key() {
        let original = attempt().compiled([1; 32], [2; 32], Some([3; 32]));
        for field in [4, 36, 52, 60, 68, HEADER, HEADER + 32, HEADER + 65] {
            let mut changed = Compiled {
                bytes: original.bytes,
            };
            changed.bytes[field] ^= 1;
            assert_ne!(changed.key(), original.key());
        }
        assert_ne!(
            attempt().compiled([1; 32], [2; 32], None).key(),
            attempt().compiled([1; 32], [2; 32], Some([0; 32])).key()
        );
    }

    #[test]
    fn unsupported_truncated_and_noncanonical_frames_fail_closed() {
        let attempt = attempt();
        let original = attempt.header[4..60].try_into().unwrap();
        let mut payload = attempt.header.to_vec();
        payload.extend_from_slice(b"snapshot");
        for length in 0..HEADER {
            assert!(Attempt::decode(&payload[..length], original).is_err());
        }
        payload[0] = 2;
        assert!(matches!(
            Attempt::decode(&payload, original),
            Err(Error::Version)
        ));
        let mut compiled = Attempt {
            header: attempt.header,
        }
        .compiled([1; 32], [2; 32], None);
        for length in [0, EVIDENCE - 1] {
            assert!(matches!(
                attempt.accept(&compiled.bytes[..length], &[1; 32]),
                Err(Error::Framing)
            ));
        }
        compiled.bytes[HEADER + 65] = 1;
        assert!(matches!(
            attempt.accept(compiled.encode(), &[1; 32]),
            Err(Error::Framing)
        ));
        compiled.bytes[HEADER + 64] = 2;
        assert!(matches!(
            attempt.accept(compiled.encode(), &[1; 32]),
            Err(Error::Framing)
        ));
    }

    #[test]
    fn terminal_outcomes_require_original_attempt_and_bounded_utf8() {
        let attempt = attempt();
        let mut status = attempt.response_header().to_vec();
        assert_eq!(attempt.accept_status(&status).unwrap(), "");
        status.extend_from_slice(b"source diagnostic");
        assert_eq!(attempt.accept_status(&status).unwrap(), "source diagnostic");
        for field in [4, 36, 52, 60, 68] {
            let mut stale = status.clone();
            stale[field] ^= 1;
            assert!(matches!(attempt.accept_status(&stale), Err(Error::Attempt)));
        }
        assert!(attempt.accept_status(&status[..HEADER - 1]).is_err());
        status.push(0xff);
        assert!(matches!(
            attempt.accept_status(&status),
            Err(Error::Framing)
        ));
    }

    #[cfg(feature = "test-fixtures")]
    #[test]
    fn child_and_parent_advance_the_same_admitted_route() {
        let evidence = attempt().compiled([1; 32], [2; 32], Some([3; 32]));
        let route = crate::profile::activation::Route::fixture(
            crate::supervisor::GenerationKey::fixture([7; 32]),
            [9; 32],
        );
        assert_eq!(
            evidence.advance_receipt([9; 32]),
            *route
                .advance(crate::supervisor::GenerationKey::resident(&evidence))
                .receipt()
                .as_bytes()
        );
    }
}
