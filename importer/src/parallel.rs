//! Independent work on each item, across the machine's cores, in order.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// `work` on each of `items`, across the machine's cores, with the results
/// in the items' order: what a loop over them gives. `None` when `work`
/// gives `None` for an item, as such a loop stops there; the items no worker
/// has started by then are left.
pub(crate) fn map<T: Sync, R: Send>(
    items: &[T],
    work: impl Fn(&T) -> Option<R> + Sync,
) -> Option<Vec<R>> {
    map_on(workers(), items, work)
}

/// [`map`] on at most `workers` threads.
fn map_on<T: Sync, R: Send>(
    workers: usize,
    items: &[T],
    work: impl Fn(&T) -> Option<R> + Sync,
) -> Option<Vec<R>> {
    let (results, _) = try_map_with_on(workers, items, || (), |(), item| work(item).ok_or(()));
    results.ok()
}

/// The machine's cores, as many threads as [`map`] works on.
pub(crate) fn workers() -> usize {
    std::thread::available_parallelism().map_or(1, NonZeroUsize::get)
}

/// `work` on each of `items` on at most `workers` threads, as [`map`] does,
/// where each worker reads its items with a state of its own that `start`
/// makes once for it. The states come back beside the results, one for each
/// worker, in no fixed order. An item whose `work` fails stops the map, and
/// the error is a loop's: that of the first item in the items' order that
/// fails, as every item before it has finished by then.
pub(crate) fn try_map_with_on<T: Sync, S: Send, R: Send, E: Send>(
    workers: usize,
    items: &[T],
    start: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, &T) -> Result<R, E> + Sync,
) -> (Result<Vec<R>, E>, Vec<S>) {
    let workers = workers.min(items.len());
    if workers <= 1 {
        let mut state = start();
        let results = items.iter().map(|item| work(&mut state, item)).collect();
        return (results, vec![state]);
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let finished = std::thread::scope(|scope| {
        let handles = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut state = start();
                    let mut done = Vec::new();
                    let mut failure = None;
                    while !failed.load(Ordering::Relaxed) {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(item) = items.get(index) else {
                            break;
                        };
                        match work(&mut state, item) {
                            Ok(result) => done.push((index, result)),
                            Err(error) => {
                                failure = Some((index, error));
                                failed.store(true, Ordering::Relaxed);
                                break;
                            }
                        }
                    }
                    (done, failure, state)
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
    let mut results = Vec::new();
    let mut first_failure: Option<(usize, E)> = None;
    let mut states = Vec::with_capacity(finished.len());
    for (done, failure, state) in finished {
        results.extend(done);
        states.push(state);
        if let Some((index, error)) = failure
            && first_failure
                .as_ref()
                .is_none_or(|(first, _)| index < *first)
        {
            first_failure = Some((index, error));
        }
    }
    if let Some((_, error)) = first_failure {
        return (Err(error), states);
    }
    results.sort_unstable_by_key(|(index, _)| *index);
    (
        Ok(results.into_iter().map(|(_, result)| result).collect()),
        states,
    )
}

/// `work` on each of `items`, taken by value, on at most `workers` threads,
/// each worker with a state of its own as in [`try_map_with_on`]. The
/// results are in the items' order; the states come back in no fixed order.
pub(crate) fn map_owned_with_on<T: Send, S: Send, R: Send>(
    workers: usize,
    items: Vec<T>,
    start: impl Fn() -> S + Sync,
    work: impl Fn(&mut S, T) -> R + Sync,
) -> (Vec<R>, Vec<S>) {
    // Each slot is read by the one worker that claims its index.
    let slots = items
        .into_iter()
        .map(|item| Mutex::new(Some(item)))
        .collect::<Vec<_>>();
    let (results, states) = try_map_with_on(workers, &slots, start, |state, slot| {
        let item = slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .expect("each index is claimed by one worker, once");
        Ok::<_, std::convert::Infallible>(work(state, item))
    });
    let Ok(results) = results;
    (results, states)
}

#[cfg(test)]
mod tests {
    use super::{map_on, map_owned_with_on, try_map_with_on};

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

    #[test]
    fn the_error_of_many_workers_is_the_first_failing_items_and_each_keeps_its_state() {
        let items = (0..1000_u32).collect::<Vec<_>>();
        let work = |seen: &mut usize, item: &u32| {
            *seen += 1;
            if *item == 300 || *item == 700 {
                Err(*item)
            } else {
                Ok(*item)
            }
        };
        let (serial, states) = try_map_with_on(1, &items, || 0_usize, work);
        assert_eq!(serial, Err(300));
        assert_eq!(states, [301]);
        let (parallel, states) = try_map_with_on(8, &items, || 0_usize, work);
        assert_eq!(parallel, Err(300));
        assert_eq!(states.len(), 8);
        let (whole, states) = try_map_with_on(8, &items[..300], || 0_usize, work);
        assert_eq!(whole, Ok(items[..300].to_vec()));
        assert_eq!(states.iter().sum::<usize>(), 300, "each item is read once");
    }

    #[test]
    fn owned_items_on_many_workers_come_back_in_item_order_each_taken_once() {
        let items = || {
            (0..1000_u32)
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        };
        let work = |seen: &mut usize, item: String| {
            *seen += 1;
            item.len()
        };
        let (serial, states) = map_owned_with_on(1, items(), || 0_usize, work);
        assert_eq!(states, [1000]);
        let (parallel, states) = map_owned_with_on(8, items(), || 0_usize, work);
        assert_eq!(serial, parallel);
        assert_eq!(
            states.iter().sum::<usize>(),
            1000,
            "each item is taken once"
        );
    }
}
