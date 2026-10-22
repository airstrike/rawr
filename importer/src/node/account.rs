//! Typed scratch accounts whose lifetime belongs to one graph evaluation.

use std::any::Any;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::{Error, GraphBudget};

#[cfg(feature = "import-baseline")]
pub(crate) mod evidence;

static NEXT: AtomicU64 = AtomicU64::new(1);

/// Stable identity only: a key never retains a graph or its account storage.
/// Stored account values must not contain a GraphBudget or graph-owning context.
pub(crate) struct Key<T> {
    id: u64,
    marker: PhantomData<fn() -> T>,
}

#[derive(Default)]
pub(super) struct Registry {
    cells: Mutex<Vec<Entry>>,
}

struct Cell<T> {
    value: Option<T>,
    refused: bool,
}

struct Entry {
    id: u64,
    value: Arc<dyn Any + Send + Sync>,
}

impl<T: Send + 'static> Key<T> {
    pub(crate) fn new() -> Result<Self, Error> {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if id == u64::MAX {
            NEXT.store(u64::MAX, Ordering::Relaxed);
            return Err(Error::State("graph account identities exhausted"));
        }
        Ok(Self {
            id,
            marker: PhantomData,
        })
    }

    /// Preserve an already-spent account; replacing an existing cell is refused.
    pub(crate) fn seed(&self, graph: &GraphBudget, value: T) -> Result<(), Error> {
        graph.accounts.insert(self.id, graph, value)
    }

    /// Initialize once in this graph, then continue the same mutable account.
    pub(crate) fn with<R>(
        &self,
        graph: &GraphBudget,
        initialize: impl FnOnce() -> Result<T, Error>,
        use_account: impl FnOnce(&mut T) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let cell = graph.accounts.get_or_insert(
            self.id,
            graph,
            Cell::<T> {
                value: None,
                refused: false,
            },
        )?;
        let mut account = cell
            .lock()
            .map_err(|_| Error::State("graph account cell was poisoned"))?;
        if account.refused {
            return Err(Error::State(
                "graph account initialization previously refused",
            ));
        }
        if account.value.is_none() {
            match initialize() {
                Ok(value) => account.value = Some(value),
                Err(error) => {
                    account.refused = true;
                    return Err(error);
                }
            }
        }
        let value = account
            .value
            .as_mut()
            .ok_or(Error::State("graph account initialization is absent"))?;
        use_account(value)
    }
}

impl Registry {
    fn insert<T: Send + 'static>(
        &self,
        id: u64,
        graph: &GraphBudget,
        value: T,
    ) -> Result<(), Error> {
        // Own the initialized cell before taking the registry lock. On refusal
        // the guard drops first, so arbitrary T destruction cannot reenter it.
        graph.decode(Self::cell_bytes::<T>()?)?;
        let value = Arc::new(Mutex::new(Cell {
            value: Some(value),
            refused: false,
        }));
        let mut cells = self
            .cells
            .lock()
            .map_err(|_| Error::State("graph account registry was poisoned"))?;
        if cells.iter().any(|entry| entry.id == id) {
            return Err(Error::State("graph account is already seeded"));
        }
        cells.try_reserve_exact(1).map_err(|_| Error::Allocation)?;
        cells.push(Entry { id, value });
        Ok(())
    }

    fn get_or_insert<T: Send + 'static>(
        &self,
        id: u64,
        graph: &GraphBudget,
        value: Cell<T>,
    ) -> Result<Arc<Mutex<Cell<T>>>, Error> {
        let mut cells = self
            .cells
            .lock()
            .map_err(|_| Error::State("graph account registry was poisoned"))?;
        if let Some(entry) = cells.iter().find(|entry| entry.id == id) {
            return Arc::clone(&entry.value)
                .downcast::<Mutex<Cell<T>>>()
                .map_err(|_| Error::State("graph account type differs"));
        }
        Self::push(&mut cells, id, graph, value)
    }

    fn cell_bytes<T>() -> Result<usize, Error> {
        std::mem::size_of::<Mutex<Cell<T>>>()
            .checked_add(2 * std::mem::size_of::<usize>())
            .and_then(|bytes| bytes.checked_add(std::mem::align_of::<Mutex<Cell<T>>>()))
            .and_then(|bytes| bytes.checked_add(std::mem::size_of::<Entry>()))
            .ok_or(Error::Allocation)
    }

    fn push<T: Send + 'static>(
        cells: &mut Vec<Entry>,
        id: u64,
        graph: &GraphBudget,
        value: Cell<T>,
    ) -> Result<Arc<Mutex<Cell<T>>>, Error> {
        // Arc's two counters, its aligned payload, and an alignment allowance
        // are admitted before allocating the erased cell. Registry slots are
        // charged on every insertion; no retirement refunds graph work.
        graph.decode(Self::cell_bytes::<T>()?)?;
        cells.try_reserve_exact(1).map_err(|_| Error::Allocation)?;
        let value = Arc::new(Mutex::new(value));
        cells.push(Entry {
            id,
            value: value.clone(),
        });
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn graph_accounts_keep_spending_and_refusal_across_returning_graphs() {
        let key = Key::<(usize, bool)>::new().unwrap();
        let a = GraphBudget::new();
        let b = GraphBudget::new();
        key.seed(&a, (7, false)).unwrap();
        let spend = |account: &mut (usize, bool)| {
            if account.1 {
                return Err(Error::State("refused"));
            }
            account.0 += 1;
            if account.0 > 8 {
                account.1 = true;
                return Err(Error::State("refused"));
            }
            Ok(account.0)
        };
        assert_eq!(key.with(&a, || panic!("seed retained"), spend).unwrap(), 8);
        assert!(key.with(&a, || panic!("seed retained"), spend).is_err());
        assert_eq!(key.with(&b, || Ok((0, false)), spend).unwrap(), 1);
        assert!(key
            .with(&a, || panic!("returning A cannot reset"), spend)
            .is_err());
        assert_eq!(
            key.with(&b, || panic!("B initializes once"), spend)
                .unwrap(),
            2
        );
        assert!(key.seed(&a, (0, false)).is_err());
    }

    #[test]
    fn graph_drop_releases_accounts_and_cell_calls_can_charge_graph() {
        struct Dropped(Arc<AtomicUsize>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
            }
        }
        let count = Arc::new(AtomicUsize::new(0));
        let key = Key::<Dropped>::new().unwrap();
        let graph = GraphBudget::new();
        key.seed(&graph, Dropped(count.clone())).unwrap();
        key.with(&graph, || panic!("seed retained"), |_| graph.hash(1))
            .unwrap();
        let sibling = graph.clone();
        drop(graph);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        drop(sibling);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    // The decoded-byte target is an accounting target. A cell seeded past
    // it is still charged to the original account and retained; the account
    // records the exceedance truthfully instead of refusing the cell.
    #[test]
    fn graph_account_storage_records_exceedance_and_retains_the_cell() {
        let graph = GraphBudget::new();
        let target = super::super::MAX_DECODED_BYTES_PER_GRAPH;
        graph.decode(target as usize).unwrap();
        assert!(
            !graph.exceedances().is_exceeded(),
            "the exact decoded-byte target is not an exceedance"
        );
        let key = Key::<usize>::new().unwrap();
        key.seed(&graph, 0)
            .expect("a cell past the accounting target is still retained");
        assert_eq!(graph.accounts.cells.lock().unwrap().len(), 1);
        let cell = Registry::cell_bytes::<usize>().unwrap() as u64;
        let decoded = graph
            .exceedances()
            .axis(super::super::DECODED_RESOURCE)
            .copied()
            .expect("the cell charge records the decoded-byte exceedance");
        assert_eq!(decoded.target, target);
        assert_eq!(decoded.first_requested, target + cell);
        assert_eq!(decoded.high_water, target + cell);
        assert!(
            decoded.site.is_some(),
            "cell charges attribute their call site"
        );
        assert_eq!(graph.usage.lock().unwrap().decoded, target + cell);
        assert_eq!(
            key.with(&graph, || panic!("seed retained"), |value| Ok(*value))
                .unwrap(),
            0
        );
        assert!(
            matches!(
                key.seed(&graph, 1),
                Err(Error::State("graph account is already seeded"))
            ),
            "identity refusal is unchanged by the accounting exceedance"
        );
        assert_eq!(graph.accounts.cells.lock().unwrap().len(), 1);
    }
}
