//! A facade's entries merged into a registry's sorted tables (#1755).
//!
//! A registry holds each table sorted. Sorting a table again after each
//! facade's install compared every entry it holds, once for each of a
//! thousand facades; a merge compares only the incoming entries, each at
//! the place a search finds for it.

use std::cmp::Ordering;

/// Merge `incoming` into `sorted`, which `compare` sorts, as a stable sort
/// of `sorted` followed by `incoming` orders them: an entry already there
/// stays ahead of an incoming one it compares equal to. Each incoming
/// entry's place, in their sorted order, is how many of the entries
/// already there go ahead of it.
pub(crate) fn merge<T>(
    sorted: &mut Vec<T>,
    mut incoming: Vec<T>,
    mut compare: impl FnMut(&T, &T) -> Ordering,
) -> Vec<usize> {
    incoming.sort_by(&mut compare);
    let places = incoming
        .iter()
        .map(|entry| sorted.partition_point(|held| compare(held, entry) != Ordering::Greater))
        .collect::<Vec<_>>();
    let held = sorted.len();
    sorted.extend(incoming);
    // Each run of incoming entries that share a place goes there in one
    // rotation of the entries after it. A facade's entries share one root,
    // so they make one run.
    let mut placed = 0;
    while placed < places.len() {
        let place = places[placed];
        let run = places[placed..]
            .iter()
            .take_while(|other| **other == place)
            .count();
        sorted[place + placed..held + placed + run].rotate_right(run);
        placed += run;
    }
    places
}

/// Where the entry at `index` before a merge is after it, given the merge's
/// `places`: one further on for each incoming entry placed at or before it.
#[cfg(feature = "rust-import")]
pub(crate) fn moved(places: &[usize], index: usize) -> usize {
    index + places.partition_point(|place| *place <= index)
}

#[cfg(all(test, feature = "rust-import"))]
mod tests {
    use super::{merge, moved};

    /// What a stable sort of the two gives, which a merge must match.
    fn sorted(held: &[(u8, char)], incoming: &[(u8, char)]) -> Vec<(u8, char)> {
        let mut all = held.to_vec();
        all.extend_from_slice(incoming);
        all.sort_by_key(|entry| entry.0);
        all
    }

    #[test]
    fn a_merge_orders_entries_as_a_stable_sort_does() {
        let held = [(1, 'a'), (3, 'b'), (3, 'c'), (5, 'd'), (9, 'e')];
        for incoming in [
            vec![],
            vec![(0, 'x')],
            vec![(10, 'x')],
            vec![(3, 'x'), (3, 'y')],
            vec![(4, 'y'), (4, 'x'), (2, 'z')],
            vec![(9, 'x'), (0, 'y'), (5, 'z'), (3, 'w'), (6, 'v')],
        ] {
            let mut merged = held.to_vec();
            merge(&mut merged, incoming.clone(), |left, right| {
                left.0.cmp(&right.0)
            });
            assert_eq!(merged, sorted(&held, &incoming), "{incoming:?}");
        }
    }

    #[test]
    fn each_held_entry_moves_past_the_incoming_ones_placed_ahead_of_it() {
        let held = [(1, 'a'), (3, 'b'), (5, 'c'), (9, 'd')];
        let incoming = vec![(3, 'x'), (0, 'y'), (6, 'z')];
        let mut merged = held.to_vec();
        let places = merge(&mut merged, incoming, |left, right| left.0.cmp(&right.0));
        assert_eq!(places, [0, 2, 3]);
        for (index, entry) in held.iter().enumerate() {
            assert_eq!(merged[moved(&places, index)], *entry);
        }
        for (order, place) in places.iter().enumerate() {
            assert!(!held.contains(&merged[place + order]));
        }
    }
}
