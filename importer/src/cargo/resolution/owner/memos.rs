//! What a run owner's saves leave for the next (D956.1, #1825).
//!
//! Each save of a long-running owner resolves the same request again, and
//! most of what it computes is what the save before it computed: Cargo's
//! view of the request, the round a cold build carried, each shared entry's
//! validation, the replay's staged interpretation and compared inputs, and
//! the API's fragment reads. The owner holds one [`Memos`] for as long as it
//! runs and lends it to each preparation, so each memo lives exactly as long
//! as the owner; a preparation lent a new one misses as a new process does.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// What one save of a run owner leaves for the next.
#[derive(Default)]
pub struct Memos {
    /// Cargo's outputs for the last few requests.
    pub(in crate::cargo::resolution) remembered: super::super::remembered::Remembered,
    /// The latest round a cold build carried to its next demand.
    pub(in crate::cargo::resolution) carried: super::carried::Latest,
    /// Each shared entry's last validation.
    pub(in crate::cargo::resolution) witnessed: super::memo::Witnessed,
    /// What replays staged, compared and recorded. An evaluation's
    /// validator reads and writes them, and the read sets of the nodes it
    /// admits hold that validator past the save and may run it on any
    /// thread, so the owner shares them with each evaluation behind one
    /// lock, which is never held across a read of a file.
    pub(in crate::cargo::resolution) replays: Arc<Mutex<Replays>>,
    /// What the API's canonicalizations read, when the owner keeps them.
    reads: Option<crate::extract::Reads>,
}

impl Memos {
    /// Keep what each save reads of the API's fragments, and the settlement
    /// made of them, for the next save (#1776): an owner canonicalizes the
    /// same fragments on each save that changes only Rawr source. A
    /// preparation that canonicalizes once reads nothing again and keeps
    /// nothing.
    pub fn keep_reads(&mut self) {
        self.reads
            .get_or_insert_with(crate::extract::Reads::default);
    }

    /// What the API's reads kept so far, lent to one preparation, when the
    /// owner keeps them.
    pub(in crate::cargo::resolution) fn reading(&mut self) -> Option<crate::extract::Reading> {
        self.reads.as_mut().map(crate::extract::Reads::reading)
    }
}

/// What replays staged, compared and recorded, which every evaluation the
/// owner lends its [`Memos`] to shares.
#[derive(Default)]
pub(in crate::cargo) struct Replays {
    /// The lock, receipt and record interpretations replays staged.
    pub(in crate::cargo::resolution) stage: super::Stage,
    /// The last replay's comparison of the recorded inputs.
    pub(in crate::cargo) compared: super::super::replay::Compared,
    /// The recorded input files of the bundles replayed.
    pub(in crate::cargo::resolution) recorded: super::recorded::Held,
}

impl Replays {
    /// The replays `shared` holds, locked. A panic under the lock leaves
    /// each memo whole, as each is replaced in one assignment, and a memo
    /// only ever answers a key it was kept under.
    pub(in crate::cargo) fn lock(shared: &Mutex<Self>) -> MutexGuard<'_, Self> {
        shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
