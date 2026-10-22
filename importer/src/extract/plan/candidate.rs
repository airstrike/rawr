//! Pure association of compiler display records with a caller-supplied namespace.
//! Neither the borrowed paths nor this result confer current filesystem authority.

use std::ffi::OsStr;
use std::path::{Component, Path};

use crate::extract::{protocol::attempt::Completion, Allocation};
use crate::node::GraphBudget;
use crate::{Error, Result};

#[derive(Clone, Copy)]
pub(crate) struct Candidate<'a> {
    pub(crate) original: &'a Path,
    pub(crate) canonical: &'a Path,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct Flags {
    pub(crate) query: bool,
    pub(crate) attempt: bool,
}

pub(crate) struct Joined {
    /// Indexed in the same order as the supplied candidate slice.
    pub(crate) flags: Vec<Flags>,
    pub(crate) conservative: bool,
}

pub(crate) fn join(
    candidates: &[Candidate<'_>],
    completion: &Completion<'_>,
    allocation: &mut Allocation,
    graph: &GraphBudget,
) -> Result<Joined> {
    let count = candidates.len();
    if count > crate::store::bundle::MAX_MEMBERS as usize {
        return Err(Error::Limit {
            resource: "compiler candidate associations",
            limit: crate::store::bundle::MAX_MEMBERS as usize,
            actual: count,
        });
    }
    let mut uncertain_display = false;
    for candidate in candidates {
        for path in [candidate.original, candidate.canonical] {
            require_absolute(path)?;
            uncertain_display |= lossy(path);
        }
    }
    for display in completion.paths().chain(completion.queries()) {
        require_absolute(Path::new(display))?;
        uncertain_display |= display.contains('\u{fffd}');
    }
    // Two sorted borrowed indices and one result row per candidate. There are
    // no copied path bytes, filesystem operations, or content hashes here.
    let bytes = count
        .checked_mul(2 * std::mem::size_of::<usize>() + std::mem::size_of::<Flags>())
        .ok_or_else(allocation_error)?;
    graph.decode(bytes).map_err(|source| Error::Node {
        operation: "compiler candidate association",
        source: Box::new(source),
    })?;
    allocation.items(count.checked_mul(3).ok_or_else(allocation_error)?)?;
    let mut originals = Vec::new();
    let mut canonical = Vec::new();
    let mut flags = Vec::new();
    originals
        .try_reserve_exact(count)
        .map_err(|_| allocation_error())?;
    canonical
        .try_reserve_exact(count)
        .map_err(|_| allocation_error())?;
    flags
        .try_reserve_exact(count)
        .map_err(|_| allocation_error())?;
    originals.extend(0..count);
    canonical.extend(0..count);
    flags.resize(count, Flags::default());
    originals.sort_unstable_by_key(|&index| candidates[index].original.as_os_str());
    canonical.sort_unstable_by_key(|&index| candidates[index].canonical.as_os_str());
    for pair in originals.windows(2) {
        if candidates[pair[0]].original.as_os_str() == candidates[pair[1]].original.as_os_str() {
            return Err(invalid("duplicate original candidate identity"));
        }
    }
    // An original spelling cannot also name a canonical target that disagrees
    // with its own mapping. Reject that ambiguity even in conservative mode.
    for candidate in candidates {
        if let Some(index) = canonical_match(&canonical, candidates, candidate.original.as_os_str())
            && candidates[index].canonical.as_os_str() != candidate.canonical.as_os_str()
        {
            return Err(invalid(
                "original and canonical candidate mappings disagree",
            ));
        }
    }
    if uncertain_display {
        flags.fill(Flags {
            query: true,
            attempt: true,
        });
        return Ok(Joined {
            flags,
            conservative: true,
        });
    }
    for display in completion.queries() {
        let key = OsStr::new(display);
        let index = originals
            .binary_search_by(|&index| candidates[index].original.as_os_str().cmp(key))
            .map_err(|_| invalid("query record has no original candidate"))?;
        flags[originals[index]].query = true;
    }
    // Mark only one representative per canonical group while reading events.
    // Repeated events must not repeatedly walk a large alias group.
    for display in completion.paths() {
        let key = OsStr::new(display);
        let original = originals
            .binary_search_by(|&index| candidates[index].original.as_os_str().cmp(key))
            .ok()
            .map(|index| originals[index]);
        let direct = canonical_match(&canonical, candidates, key);
        let target = original
            .or(direct)
            .ok_or_else(|| invalid("attempt record has no candidate"))?;
        if let (Some(original), Some(direct)) = (original, direct)
            && candidates[original].canonical.as_os_str()
                != candidates[direct].canonical.as_os_str()
        {
            return Err(invalid("attempt record names different native targets"));
        }
        let representative = canonical_match(
            &canonical,
            candidates,
            candidates[target].canonical.as_os_str(),
        )
        .ok_or_else(|| invalid("candidate canonical group absent"))?;
        flags[representative].attempt = true;
    }
    let mut position = 0;
    while position < count {
        let first = canonical[position];
        let target = candidates[first].canonical.as_os_str();
        let attempt = flags[first].attempt;
        while position < count && candidates[canonical[position]].canonical.as_os_str() == target {
            flags[canonical[position]].attempt = attempt;
            position += 1;
        }
    }
    if completion.conservative {
        flags.fill(Flags {
            query: true,
            attempt: true,
        });
    }
    Ok(Joined {
        flags,
        conservative: completion.conservative,
    })
}

fn canonical_match(indices: &[usize], candidates: &[Candidate<'_>], key: &OsStr) -> Option<usize> {
    let start = indices.partition_point(|&index| candidates[index].canonical.as_os_str() < key);
    indices
        .get(start)
        .copied()
        .filter(|&index| candidates[index].canonical.as_os_str() == key)
}

fn require_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("candidate path is relative or traverses a parent"));
    }
    Ok(())
}

fn lossy(path: &Path) -> bool {
    path.to_str().is_none_or(|path| path.contains('\u{fffd}'))
}

fn allocation_error() -> Error {
    Error::Node {
        operation: "compiler candidate association",
        source: Box::new(crate::node::Error::Allocation),
    }
}

fn invalid(reason: &'static str) -> Error {
    Error::Extraction {
        code: "compiler-candidate-association",
        reason: reason.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::protocol::attempt::{self, Invocation};
    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn wire(attempts: &[&str], queries: &[&str], conservative: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        attempt::encode(
            &mut bytes,
            ID,
            conservative,
            &[Invocation {
                nonce: ID,
                correlation: ID,
            }],
            attempts.iter().copied(),
            queries.iter().copied(),
        )
        .unwrap();
        bytes
    }
    fn candidate<'a>(original: &'a str, canonical: &'a str) -> Candidate<'a> {
        Candidate {
            original: Path::new(original),
            canonical: Path::new(canonical),
        }
    }
    fn run(candidates: &[Candidate<'_>], bytes: &[u8]) -> Result<Joined> {
        join(
            candidates,
            &Completion::decode(bytes).unwrap(),
            &mut Allocation::with_limits(usize::MAX, usize::MAX),
            &GraphBudget::new(),
        )
    }
    #[test]
    fn aliases_and_rejected_queries_retain_exact_roles() {
        let candidates = [
            candidate("/alias.rlib", "/native.rlib"),
            candidate("/other.rlib", "/native.rlib"),
            candidate("/rejected.rmeta", "/rejected.rmeta"),
        ];
        let joined = run(
            &candidates,
            &wire(
                &["/native.rlib", "/alias.rlib"],
                &["/rejected.rmeta"],
                false,
            ),
        )
        .unwrap();
        assert!(!joined.conservative);
        assert_eq!(
            joined.flags,
            [
                Flags {
                    query: false,
                    attempt: true
                },
                Flags {
                    query: false,
                    attempt: true
                },
                Flags {
                    query: true,
                    attempt: false
                }
            ]
        );
        assert!(run(&candidates, &wire(&[], &["/native.rlib"], false)).is_err());
    }
    #[test]
    fn malformed_and_ambiguous_associations_reject() {
        assert!(run(
            &[candidate("/a", "/b"), candidate("/c", "/a")],
            &wire(&[], &[], false)
        )
        .is_err());
        assert!(run(
            &[candidate("/a", "/a"), candidate("/a", "/a")],
            &wire(&[], &[], false)
        )
        .is_err());
        assert!(run(&[candidate("relative", "/a")], &wire(&[], &[], true)).is_err());
        assert!(run(&[candidate("/a/../b", "/b")], &wire(&[], &[], false)).is_err());
        assert!(run(&[candidate("/a", "/a")], &wire(&["/absent"], &[], false)).is_err());
    }
    #[test]
    fn lossy_or_explicit_fallback_covers_every_candidate() {
        use std::os::unix::ffi::OsStrExt;
        let native = Path::new(OsStr::from_bytes(b"/bad-\xff"));
        let candidates = [
            Candidate {
                original: native,
                canonical: native,
            },
            candidate("/other", "/other"),
        ];
        let joined = run(&candidates, &wire(&[], &[], false)).unwrap();
        assert!(joined.conservative);
        assert!(joined.flags.iter().all(|row| row.query && row.attempt));
        assert!(
            run(
                &[candidate("/literal-\u{fffd}", "/literal-\u{fffd}")],
                &wire(&[], &[], false)
            )
            .unwrap()
            .conservative
        );
        assert!(run(&[candidate("/a", "/a")], &wire(&["/unmatched"], &[], true)).is_err());
        assert!(
            run(&[candidate("/a", "/a")], &wire(&["/a"], &[], true))
                .unwrap()
                .conservative
        );
    }
    #[test]
    fn original_spent_accounts_admit_exactly_and_record_one_less_exceedance() {
        let candidates = [candidate("/a", "/a")];
        let bytes = wire(&[], &["/a"], false);
        let completion = Completion::decode(&bytes).unwrap();
        let required = 2 * std::mem::size_of::<usize>() + std::mem::size_of::<Flags>();
        for short in [false, true] {
            let graph = GraphBudget::new();
            graph
                .decode(
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize - required
                        + usize::from(short),
                )
                .unwrap();
            let mut allocation = Allocation::with_limits(10, 0);
            allocation.items(7).unwrap();
            // The graph decode target never refuses. The exact floor
            // leaves no exceedance; one byte short records the decoded axis
            // with a high water of exactly target + 1, proving the charge is
            // exactly `required` bytes.
            join(&candidates, &completion, &mut allocation, &graph).unwrap();
            // The exact canonical item floor (7 + 3) records nothing.
            assert_eq!(allocation.usage().0, 10);
            assert!(!allocation.exceedances().is_exceeded());
            let exceeded = graph.exceedances();
            assert_eq!(exceeded.is_exceeded(), short);
            if short {
                let axis = exceeded.axis(crate::node::DECODED_RESOURCE).unwrap();
                assert_eq!(axis.target, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
                assert_eq!(
                    axis.first_requested,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1
                );
                assert_eq!(
                    axis.high_water,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH + 1
                );
            } else {
                assert_eq!(
                    graph.census_usage().unwrap().0,
                    crate::node::MAX_DECODED_BYTES_PER_GRAPH
                );
            }
        }
        // The canonical item target is recorded, never refused. One
        // short of the exact association charge still joins and records the
        // item axis at exactly the cumulative charge of 7 + 3.
        let mut allocation = Allocation::with_limits(9, 0);
        allocation.items(7).unwrap();
        join(
            &candidates,
            &completion,
            &mut allocation,
            &GraphBudget::new(),
        )
        .expect("an item target records instead of refusing the association");
        assert_eq!(allocation.usage().0, 10);
        assert!(!allocation.is_refused());
        let axis = *allocation
            .exceedances()
            .axis("canonical allocation items")
            .expect("the item axis records");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            (9, 10, 10)
        );
        // Terminal only through a genuine cause: a latched account joins nothing.
        assert!(matches!(
            join(
                &candidates,
                &completion,
                &mut Allocation::refused(),
                &GraphBudget::new()
            ),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
    }
}
