//! Late evidence charges share original usage, without retaining graph scratch.

use std::sync::{Arc, Mutex};

use crate::metrics::accounting::{Axis, Exceedance, Graph, Stage};
use crate::node::{Error, GraphBudget, GraphUsage, DECODED_RESOURCE, HASHED_RESOURCE};

#[derive(Clone)]
pub(crate) struct Usage {
    original: Arc<Mutex<GraphUsage>>,
    /// The original graph's process-unique account identity.
    account: u64,
    decoded_target: u64,
    hashed_target: u64,
}

impl Usage {
    pub(crate) fn retain(graph: &GraphBudget) -> Self {
        Self {
            original: graph.usage.clone(),
            account: graph.id,
            decoded_target: graph.targets.decoded,
            hashed_target: graph.targets.hashed,
        }
    }

    pub(crate) fn shares_with(&self, graph: &GraphBudget) -> bool {
        Arc::ptr_eq(&self.original, &graph.usage)
    }

    pub(crate) fn shares_usage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.original, &other.original)
    }

    /// The caller retains this returned observation even after metrics scopes end.
    pub(crate) fn charge(&self, bytes: usize) -> (Result<(), Error>, Option<Graph>) {
        self.charge_axis(
            bytes,
            |usage| &mut usage.decoded,
            Axis::Decoded,
            self.decoded_target,
            DECODED_RESOURCE,
        )
    }

    pub(crate) fn hash(&self, bytes: usize) -> (Result<(), Error>, Option<Graph>) {
        self.charge_axis(
            bytes,
            |usage| &mut usage.hashed,
            Axis::Hashed,
            self.hashed_target,
            HASHED_RESOURCE,
        )
    }

    // Late evidence charges continue the original graph account. Crossing
    // the target records the axis on that account and the charge still
    // succeeds; only checked-arithmetic overflow is terminal.
    fn charge_axis(
        &self,
        bytes: usize,
        select: fn(&mut GraphUsage) -> &mut u64,
        axis: Axis,
        target: u64,
        resource: &'static str,
    ) -> (Result<(), Error>, Option<Graph>) {
        let Ok(mut usage) = self.original.lock() else {
            return (
                Err(Error::State("original evidence account poisoned")),
                None,
            );
        };
        let current = select(&mut usage);
        let requested = u64::try_from(bytes)
            .ok()
            .and_then(|bytes| current.checked_add(bytes));
        let (result, exceeded) = match requested {
            Some(actual) => {
                *current = actual;
                let exceeded =
                    actual > target && usage.exceeded.exceeded(resource, target, actual, None);
                (Ok(()), exceeded)
            }
            None => (Err(Error::Allocation), false),
        };
        let observation = Graph {
            account: self.account,
            stage: Stage::Charge(axis),
            admitted: [
                usage.lookups as u64,
                usage.comparisons as u64,
                usage.deep as u64,
                usage.decoded,
                usage.paths as u64,
                usage.hashed,
            ],
            exceedance: requested
                .is_none_or(|amount| amount > target)
                .then_some(Exceedance {
                    axis,
                    requested,
                    limit: target,
                }),
        };
        let record = usage.exceeded;
        drop(usage);
        if exceeded {
            crate::exceedance::publish(crate::exceedance::Account::Graph, self.account, &record);
        }
        (result, Some(observation))
    }
}
