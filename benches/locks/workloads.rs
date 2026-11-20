//! Every workload, by name, in the order a run measures them.

mod futures;
mod mutex;
mod recompile;

use crate::workload::Workload;

/// Every workload this build runs, in the order a run measures them. The
/// handoff needs the counters, which tell the holder its waiter is parked;
/// `futures` comes last, because it leaves this process's owner thread
/// driven for good.
pub const NAMES: &[&str] = &[
    mutex::CONTENDED,
    #[cfg(feature = "counters")]
    mutex::HANDOFF,
    recompile::NAME,
    futures::NAME,
];

/// The workload `name`, prepared: compiled and set up, so that what is
/// left is the path it measures.
pub fn prepare(name: &str) -> Option<Workload> {
    Some(match name {
        mutex::CONTENDED => mutex::contended(),
        #[cfg(feature = "counters")]
        mutex::HANDOFF => mutex::handoff(),
        recompile::NAME => recompile::workload(),
        futures::NAME => futures::workload(),
        _ => return None,
    })
}
