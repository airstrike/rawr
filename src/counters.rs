//! How often each lock the locks review named is taken, for the locks
//! bench (`benches/locks`). Only a `lock-counters` build has this module;
//! the `counted!` call at each site expands to nothing in any other.
//!
//! A count is a relaxed add on a cache line of its own, so counting one
//! site never contends with counting another.

use std::sync::atomic::{AtomicU64, Ordering};

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
    /// C: `served()` reading `DRIVER`.
    RuntimeServed,
    /// C: `drive()` setting `DRIVER`.
    RuntimeDrive,
    /// C: a `Made<T>` or `Slot` cell of a native output.
    RuntimeMade,
    /// C1: a `thread::Builder` taken from its place.
    ThreadBuilder,
    /// B3: a host value's `==` map.
    HostEqual,
    /// B3: a host value's `<` map.
    HostCompare,
    /// B3: a host value's `{:?}` map.
    HostDebug,
    /// B3: a host value's `{}` map.
    HostDisplay,
    /// A3: `place::reading`'s registry.
    PlaceReading,
    /// B4: the import account's refusal latch.
    AccountLatch,
    /// A1: the resident compile caches on `Engine`.
    ResidentCache,
    /// B6: one compile's native-miss record.
    CaptureTerminal,
    /// D1: the inspector's locator.
    InspectLocate,
    /// D1: a hosted inspector's compiled files.
    InspectFiles,
}

impl Site {
    /// Every site, in the order a result lists them.
    pub const ALL: [Self; 17] = [
        Self::CellLock,
        Self::CellWait,
        Self::CellRelease,
        Self::RuntimeServed,
        Self::RuntimeDrive,
        Self::RuntimeMade,
        Self::ThreadBuilder,
        Self::HostEqual,
        Self::HostCompare,
        Self::HostDebug,
        Self::HostDisplay,
        Self::PlaceReading,
        Self::AccountLatch,
        Self::ResidentCache,
        Self::CaptureTerminal,
        Self::InspectLocate,
        Self::InspectFiles,
    ];

    /// The site's name in a result.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::CellLock => "cell.lock",
            Self::CellWait => "cell.wait",
            Self::CellRelease => "cell.release",
            Self::RuntimeServed => "runtime.served",
            Self::RuntimeDrive => "runtime.drive",
            Self::RuntimeMade => "runtime.made",
            Self::ThreadBuilder => "thread.builder",
            Self::HostEqual => "host.equal",
            Self::HostCompare => "host.compare",
            Self::HostDebug => "host.debug",
            Self::HostDisplay => "host.display",
            Self::PlaceReading => "place.reading",
            Self::AccountLatch => "account.latch",
            Self::ResidentCache => "resident.cache",
            Self::CaptureTerminal => "capture.terminal",
            Self::InspectLocate => "inspect.locate",
            Self::InspectFiles => "inspect.files",
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

/// Count one taking of `site`'s lock.
pub(crate) fn bump(site: Site) {
    COUNTS[site as usize].0.fetch_add(1, Ordering::Relaxed);
}

/// Start every count again from zero.
pub fn reset() {
    for count in &COUNTS {
        count.0.store(0, Ordering::Relaxed);
    }
}
