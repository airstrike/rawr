//! One in-flight owner across an exact branch of pure staged node evaluation.

use super::*;
use crate::node::read_set::Retention;
use crate::node::replay::Staged;

#[cfg(test)]
mod tests;

pub(crate) struct Batch<'a> {
    ownership: &'a Ownership<'a>,
    store: &'a Store,
    retained: Retained<'a>,
    guard: Option<PinGuard>,
    sink: &'a dyn crate::metrics::Sink,
    hits: [u64; Kind::Runner as usize + 1],
}

enum Retained<'a> {
    Initial(&'a Retention),
    Successor(Retention),
}

impl Retained<'_> {
    fn get(&self) -> &Retention {
        match self {
            Self::Initial(value) => value,
            Self::Successor(value) => value,
        }
    }
}

impl<'a> Batch<'a> {
    pub(super) fn begin(
        store: &'a Store,
        initial: &'a Retention,
        sink: &'a dyn crate::metrics::Sink,
        ownership: &'a Ownership<'a>,
    ) -> Result<Self, Error> {
        let guard = lease::with_project(&store.root, store.project, || {
            store.retain_locked(ownership, initial)
        })?;
        Ok(Self {
            ownership,
            store,
            retained: Retained::Initial(initial),
            guard: Some(guard),
            sink,
            hits: [0; Kind::Runner as usize + 1],
        })
    }

    pub(crate) fn evaluate<K: Value>(
        &mut self,
        recipe: &Direct<K>,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Staged<K>, Error> {
        self.retained.get().covers_parents(recipe.parents())?;
        let store = self.store;
        let mut rejected = false;
        if let Some(value) = self.lookup(recipe, &mut rejected)? {
            let value = lease::with_project(&store.root, store.project, || self.advance(value))?;
            self.hits[K::KIND as usize] += 1;
            return Ok(value);
        }
        self.execute(recipe, execute, rejected)
    }

    /// Resume a consumed lookup disposition. The next lookup is the required
    /// authoritative recheck after acquiring action/project ownership.
    pub(crate) fn resume<K: Value>(
        &mut self,
        recipe: &Direct<K>,
        miss: crate::node::Miss,
        execute: impl FnOnce() -> Result<K, Error>,
    ) -> Result<Staged<K>, Error> {
        self.retained.get().covers_parents(recipe.parents())?;
        self.execute(recipe, execute, miss.recovery_required())
    }

    fn execute<K: Value>(
        &mut self,
        recipe: &Direct<K>,
        execute: impl FnOnce() -> Result<K, Error>,
        mut rejected: bool,
    ) -> Result<Staged<K>, Error> {
        let store = self.store;
        let sink = self.sink;
        store.with_action_lease(recipe.action(), || {
            lease::with_project(&store.root, store.project, || {
                if let Some(value) = self.lookup(recipe, &mut rejected)? {
                    let value = self.advance(value)?;
                    self.hits[K::KIND as usize] += 1;
                    return Ok(value);
                }
                // This batch already owns the parent pin. Reservation must not
                // create and release a second owner of the single InFlight role.
                let value = store.with_node_reservation_locked(
                    self.ownership,
                    accounting::LeaseKey::Action(recipe.action()),
                    recipe.output_limit(),
                    recipe.parent_count(),
                    None,
                    || {
                        sink.node(K::KIND.metric(), crate::metrics::NodeEvent::ExecutedMiss);
                        let bytes = execute()?.into_encoded(recipe.output_limit())?;
                        if bytes.len() > recipe.output_limit() {
                            return Err(Error::Limit {
                                resource: "node content bytes",
                                limit: recipe.output_limit(),
                                actual: bytes.len(),
                            });
                        }
                        Ok(Reserved::Publish {
                            result: (),
                            recipe: recipe.clone(),
                            bytes,
                            bundle: None,
                        })
                    },
                    |()| {
                        sink.node(K::KIND.metric(), crate::metrics::NodeEvent::Published);
                        store
                            .lookup_staged(recipe, store.graph_budget())?
                            .ok_or(Error::State("published batch node disappeared"))
                    },
                )?;
                self.advance(value)
            })
        })
    }

    /// Acquired hit bytes and their recipe are reused verbatim; admission checks
    /// storage under the new pin but does not repeat semantic lookup or decode.
    #[cfg(test)]
    pub(crate) fn carry<K: Value>(
        &mut self,
        value: crate::node::Validated<K>,
    ) -> Result<crate::node::staging::Owned<K>, Error> {
        self.carry_owned(crate::node::staging::Owned::Current(value))
    }

    pub(crate) fn carry_owned<K: Value>(
        &mut self,
        value: crate::node::staging::Owned<K>,
    ) -> Result<crate::node::staging::Owned<K>, Error> {
        use crate::node::staging::Owned;
        let staged_hit = matches!(&value, Owned::Staged(_));
        let successor = match &value {
            Owned::Current(value) => self.retained.get().extend_current(value)?,
            Owned::Staged(value) => self.retained.get().extend_staged(value)?,
        };
        let store = self.store;
        lease::with_project(&store.root, store.project, || self.replace(successor))?;
        if staged_hit {
            self.hits[K::KIND as usize] += 1;
        }
        Ok(value)
    }

    pub(crate) fn retention(&self) -> &Retention {
        self.retained.get()
    }

    fn lookup<K: Value>(
        &self,
        recipe: &Direct<K>,
        rejected: &mut bool,
    ) -> Result<Option<Staged<K>>, Error> {
        match self.store.lookup_staged(recipe, self.store.graph_budget()) {
            Ok(value) => Ok(value),
            Err(Error::CleanupRequired(_)) => {
                if !*rejected {
                    self.sink
                        .node(K::KIND.metric(), crate::metrics::NodeEvent::RejectedHit);
                    *rejected = true;
                }
                Ok(None)
            }
            Err(error) => {
                self.sink
                    .node(K::KIND.metric(), crate::metrics::NodeEvent::RejectedHit);
                Err(error)
            }
        }
    }

    fn advance<K: Value>(&mut self, value: Staged<K>) -> Result<Staged<K>, Error> {
        let successor = self.retained.get().extend_staged(&value)?;
        self.replace(successor)?;
        Ok(value)
    }

    fn replace(&mut self, successor: Retention) -> Result<(), Error> {
        // Transactional acquisition retains the predecessor on failure. On
        // success this batch alone owns release of the new ledger role.
        let guard = self.store.retain_locked(self.ownership, &successor)?;
        let previous = self.guard.replace(guard);
        self.retained = Retained::Successor(successor);
        drop(previous);
        Ok(())
    }

    pub(super) fn finish<T>(mut self, result: Result<T, Error>) -> Result<T, Error> {
        let store = self.store;
        let value = lease::with_project(&store.root, store.project, || {
            store.finish_in_flight(
                self.ownership,
                Some(InFlight::Retained(self.retained.get())),
                self.guard.take(),
                result,
            )
        })?;
        for (code, count) in self.hits.iter().enumerate() {
            if let Some(kind) = Kind::from_code(code as u8) {
                for _ in 0..*count {
                    self.sink
                        .node(kind.metric(), crate::metrics::NodeEvent::ValidatedHit);
                }
            }
        }
        Ok(value)
    }
}
