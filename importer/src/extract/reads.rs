//! What a run owner keeps of its API's canonicalizations between saves
//! (#1776, #1786).
//!
//! A canonicalization reads each fragment's body and settles the candidate
//! they make, and the next save that changed no crate's API reads and
//! settles the same. The owner's [`Reads`] holds what the last ones kept,
//! and lends each preparation a [`Reading`] of it. A canonicalization reads
//! the kept bodies and settlement from its reading without taking them, so
//! two at once, a save's and a background read's, both find them; it sends
//! what it read back, and the owner takes that in before it lends the next
//! reading.

use std::collections::BTreeMap;
use std::sync::{mpsc, Arc};

use crate::cargo;

use super::{fragment, settled, Catalog};

/// What an owner's canonicalizations kept, and the channel each sends what
/// it read on.
pub(crate) struct Reads {
    bodies: Option<Arc<fragment::Kept>>,
    settled: Option<Arc<settled::Kept>>,
    sent: mpsc::Receiver<Read>,
    send: mpsc::Sender<Read>,
}

/// One canonicalization's reads, sent for the next.
enum Read {
    Bodies(fragment::Kept),
    Settled(settled::Kept),
}

/// What one preparation's canonicalizations read the kept bodies and
/// settlement from, and send what they read to.
#[derive(Clone)]
pub(crate) struct Reading {
    bodies: Option<Arc<fragment::Kept>>,
    settled: Option<Arc<settled::Kept>>,
    send: mpsc::Sender<Read>,
}

impl Default for Reads {
    fn default() -> Self {
        let (send, sent) = mpsc::channel();
        Self {
            bodies: None,
            settled: None,
            sent,
            send,
        }
    }
}

impl Reads {
    /// What the canonicalizations kept, with each one sent since the last
    /// reading taken in, lent to one preparation.
    pub(crate) fn reading(&mut self) -> Reading {
        for read in self.sent.try_iter() {
            match read {
                Read::Bodies(bodies) => self.bodies = Some(Arc::new(bodies)),
                Read::Settled(settled) => self.settled = Some(Arc::new(settled)),
            }
        }
        Reading {
            bodies: self.bodies.clone(),
            settled: self.settled.clone(),
            send: self.send.clone(),
        }
    }
}

impl Reading {
    /// The bodies kept by read key, when they were read against `catalog`
    /// and `limits`.
    pub(super) fn bodies(
        &self,
        catalog: &Catalog,
        limits: &cargo::Limits,
    ) -> Option<&BTreeMap<[u8; 32], Arc<fragment::Body>>> {
        self.bodies.as_deref()?.of(catalog, limits)
    }

    /// The settlement kept of the fragments whose read keys are `keys`, in
    /// order, when it was made under `catalog`, `limits` and the policy
    /// whose identity is `policy`.
    pub(super) fn settled(
        &self,
        keys: &[[u8; 32]],
        catalog: &Catalog,
        limits: &cargo::Limits,
        policy: &[u8; 32],
    ) -> Option<&settled::Settled> {
        self.settled.as_deref()?.of(keys, catalog, limits, policy)
    }

    /// Keep `bodies` for the next preparation, in place of those kept.
    pub(super) fn keep_bodies(&self, bodies: fragment::Kept) {
        // An owner that has gone keeps nothing.
        let _ = self.send.send(Read::Bodies(bodies));
    }

    /// Keep `settled` for the next preparation, in place of the one kept.
    pub(super) fn keep_settled(&self, settled: settled::Kept) {
        let _ = self.send.send(Read::Settled(settled));
    }
}
