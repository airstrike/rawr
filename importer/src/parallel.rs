//! Independent work on each item, across the machine's cores, in order.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// `work` on each of `items`, across the machine's cores, with the results
/// in the items' order: what a loop over them gives. `None` when `work`
/// gives `None` for an item, as such a loop stops there; the items no worker
/// has started by then are left.
pub(crate) fn map<T: Sync, R: Send>(
    items: &[T],
    work: impl Fn(&T) -> Option<R> + Sync,
) -> Option<Vec<R>> {
    let workers = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);
    map_on(workers, items, work)
}

/// [`map`] on at most `workers` threads.
fn map_on<T: Sync, R: Send>(
    workers: usize,
    items: &[T],
    work: impl Fn(&T) -> Option<R> + Sync,
) -> Option<Vec<R>> {
    let workers = workers.min(items.len());
    if workers <= 1 {
        return items.iter().map(work).collect();
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let finished = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut done = Vec::new();
                    while !failed.load(Ordering::Relaxed) {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        match work(item) {
                            Some(result) => done.push((index, result)),
                            None => failed.store(true, Ordering::Relaxed),
                        }
                    }
                    done
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
            })
            .collect::<Vec<_>>()
    });
    if failed.into_inner() {
        return None;
    }
    let mut results = finished.into_iter().flatten().collect::<Vec<_>>();
    results.sort_unstable_by_key(|(index, _)| *index);
    Some(results.into_iter().map(|(_, result)| result).collect())
}

#[cfg(test)]
mod tests {
    use super::map_on;

    #[test]
    fn results_on_many_workers_are_the_results_of_one_in_item_order() {
        let items = (0..1000_u64).collect::<Vec<_>>();
        let work = |item: &u64| {
            // Uneven work, so the workers finish out of order.
            let rounds = item % 7 * 1000;
            Some((0..rounds).fold(*item, |hash, round| {
                hash.wrapping_mul(0x0100_0000_01b3).wrapping_add(round)
            }))
        };
        let serial = map_on(1, &items, work).expect("no item fails");
        let parallel = map_on(8, &items, work).expect("no item fails");
        assert_eq!(serial, parallel);
        assert_eq!(serial.len(), items.len());
    }

    #[test]
    fn an_item_that_fails_fails_the_whole_map() {
        let items = (0..100_u32).collect::<Vec<_>>();
        let work = |item: &u32| (*item != 37).then_some(*item);
        assert!(map_on(1, &items, work).is_none());
        assert!(map_on(8, &items, work).is_none());
    }
}
