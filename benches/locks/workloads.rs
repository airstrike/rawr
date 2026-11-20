//! Every workload, by name, in the order a run measures them.

#[cfg(unix)]
mod debugger;
mod futures;
mod host_values;
mod mutex;
mod recompile;

use crate::workload::Workload;

/// Every workload's name, in the order a run measures them. `futures-driven`
/// comes last: it makes this process's owner thread a driven one for good,
/// which no later workload may see.
pub const NAMES: &[&str] = &[
    mutex::CONTENDED,
    mutex::HANDOFF,
    host_values::NAME,
    recompile::NAME,
    #[cfg(unix)]
    debugger::NAME,
    futures::UNDRIVEN,
    futures::DRIVEN,
];

/// The workload `name`, prepared: compiled and set up, so that what is
/// left is the path it measures.
pub fn prepare(name: &str) -> Option<Workload> {
    Some(match name {
        mutex::CONTENDED => mutex::contended(),
        mutex::HANDOFF => mutex::handoff(),
        host_values::NAME => host_values::workload(),
        recompile::NAME => recompile::workload(),
        #[cfg(unix)]
        debugger::NAME => debugger::workload(),
        futures::UNDRIVEN => futures::undriven(),
        futures::DRIVEN => futures::driven(),
        _ => return None,
    })
}
