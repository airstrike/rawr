//! How often the locks review's B5 and A1 locks are taken, for the locks
//! bench (`benches/locks`). Only a `lock-counters` build has this module;
//! the `counted!` call at each site expands to nothing in any other.
//!
//! Each count moves when its fix removes work: a wait that ends at a poll,
//! a guard that never releases, a lock of a cache that leaves `Engine`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

/// One counted lock site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Site {
    /// B5: a script `Mutex::lock`.
    CellLock,
    /// B5: one wait of a script `Mutex::lock` whose lock was held, which
    /// ends at a release or at the 1 ms poll.
    CellWait,
    /// B5: a script `Mutex` guard's release, which wakes one waiter.
    CellRelease,
    /// A1: a lock of the resident compile caches on `Engine`.
    ResidentCache,
}

impl Site {
    /// Every site, in the order a result lists them.
    pub const ALL: [Self; 4] = [
        Self::CellLock,
        Self::CellWait,
        Self::CellRelease,
        Self::ResidentCache,
    ];

    /// The site's name in a result.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CellLock => "cell.lock",
            Self::CellWait => "cell.wait",
            Self::CellRelease => "cell.release",
            Self::ResidentCache => "resident.cache",
        }
    }

    /// How many times this site's lock was taken since the last
    /// [`reset`].
    #[must_use]
    pub fn count(self) -> u64 {
        COUNTS[self as usize].0.load(Ordering::Relaxed)
    }
}

/// One site's count, alone on its cache line.
#[repr(align(128))]
struct Count(AtomicU64);

static COUNTS: [Count; Site::ALL.len()] = [const { Count(AtomicU64::new(0)) }; Site::ALL.len()];

/// What a [`wait_above`] waits on: told each time a script `Mutex::lock`
/// starts a wait, under the lock's gate.
static WAITS: (Mutex<()>, Condvar) = (Mutex::new(()), Condvar::new());

/// Count one taking of `site`'s lock. A `CellWait` also wakes every
/// [`wait_above`].
pub(crate) fn bump(site: Site) {
    COUNTS[site as usize].0.fetch_add(1, Ordering::Relaxed);
    if site == Site::CellWait {
        let _told = WAITS.0.lock().unwrap_or_else(PoisonError::into_inner);
        WAITS.1.notify_all();
    }
}

/// Wait for `site` to count more than `count`: for `CellWait`, for a
/// script `Mutex::lock` to wait, which a holder waits for to know its
/// waiter is parked. It waits on the count's event, never on a clock.
///
/// # Panics
///
/// For a site other than `CellWait`, whose count wakes no one.
pub fn wait_above(site: Site, count: u64) {
    assert_eq!(site, Site::CellWait, "only a wait's count is told");
    let mut told = WAITS.0.lock().unwrap_or_else(PoisonError::into_inner);
    while site.count() <= count {
        told = WAITS.1.wait(told).unwrap_or_else(PoisonError::into_inner);
    }
}

/// Start every count again from zero.
pub fn reset() {
    for count in &COUNTS {
        count.0.store(0, Ordering::Relaxed);
    }
}
