//! Borrowed canonical profile lookup without another JSON interpretation.

use crate::cargo::unit::{ArtifactProfile, BuildGraph};
use crate::input::dependency::observed::index;
use crate::input::stored::observe::slots::Slots;
use crate::input::stored::{Budget, Error, Result};

pub(super) struct Profiles<'graph> {
    values: Slots<&'graph ArtifactProfile>,
    index: index::Index,
}

impl<'graph> Profiles<'graph> {
    pub(super) fn bind(graph: &'graph BuildGraph, budget: &mut Budget<'_>) -> Result<Self> {
        budget.check()?;
        let mut profiles = Self {
            values: Slots::new(),
            index: index::Index::new(),
        };
        for output in graph.artifacts() {
            budget.check()?;
            let profile = output.artifact_profile();
            if let Ok(vacant) = profiles
                .index
                .locate(profiles.values.as_slice(), |candidate| {
                    candidate.as_bytes().cmp(profile.as_bytes())
                })
            {
                profiles.values.push(profile, budget)?;
                profiles.index.insert(vacant, budget)?;
            }
        }
        Ok(profiles)
    }

    /// Equality supplies the profile grammar; the caller separately joins artifacts.
    pub(super) fn decode(&self, bytes: &[u8], budget: &mut Budget<'_>) -> Result<ArtifactProfile> {
        budget.check()?;
        let position = self
            .index
            .locate(self.values.as_slice(), |candidate| {
                candidate.as_bytes().cmp(bytes)
            })
            .err()
            .ok_or(Error::Encoding(
                "stored artifact profile is absent from unit graph",
            ))?;
        budget.check()?;
        let mut owned = budget.owned(bytes.len())?;
        owned.extend_from_slice(bytes);
        let owned = budget.owned_box(owned)?;
        self.values.as_slice()[position]
            .retain_exact(owned)
            .map_err(Error::from)
    }
}

#[cfg(test)]
mod tests {
    use super::Profiles;
    use crate::cargo::{unit, Limits};
    use crate::input::stored::{Budget, Error};
    use crate::input::{Allocation, Usage};
    use crate::node;

    const PRIOR: Usage = Usage {
        items: 7,
        bytes: 31,
    };

    fn graph() -> unit::BuildGraph {
        unit::BuildGraph::fixture_artifacts(Box::from([
            unit::Output::fixture("pkg", "/src/lib.rs", "first", "lib", "lib"),
            unit::Output::fixture("pkg", "/src/lib.rs", "second", "lib", "lib"),
        ]))
    }

    #[test]
    fn duplicate_profiles_borrow_one_entry_and_decode_exact_bytes() {
        let units = graph();
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all paths spent");
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .expect("all hashing spent");
        graph.exhaust_comparisons();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let profiles = Profiles::bind(&units, &mut budget).expect("borrowed profile index");
        assert_eq!(profiles.values.as_slice().len(), 1);
        let expected = units.artifacts()[0].artifact_profile();
        assert!(std::ptr::eq(profiles.values.as_slice()[0], expected));
        let actual = profiles
            .decode(expected.as_bytes(), &mut budget)
            .expect("known profile");
        assert_eq!(actual, *expected);
        assert_ne!(actual.as_bytes().as_ptr(), expected.as_bytes().as_ptr());
        let owned = expected.as_bytes().to_vec().into_boxed_slice();
        let pointer = owned.as_ptr();
        assert_eq!(
            expected
                .retain_exact(owned)
                .expect("admitted data moves")
                .as_bytes()
                .as_ptr(),
            pointer
        );
        assert!(expected.retain_exact(Box::from(b"{}".as_slice())).is_err());
    }

    #[test]
    fn unknown_profiles_reject_before_ownership_or_parser_work() {
        let units = graph();
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        let profiles = Profiles::bind(
            &units,
            &mut Budget::borrow(&limits, &mut allocation, &graph),
        )
        .expect("profile index");
        let before = allocation.usage();
        for bytes in [b"{}".as_slice(), b"null", b"{\"debuginfo\":[]}", b"\xff"] {
            assert!(matches!(
                profiles.decode(bytes, &mut Budget::borrow(&limits, &mut allocation, &graph)),
                Err(Error::Encoding(
                    "stored artifact profile is absent from unit graph"
                ))
            ));
            assert_eq!(allocation.usage(), before);
        }
    }

    #[test]
    fn index_and_profile_copy_keep_the_original_exact_account() {
        let units = graph();
        let limits = Limits::standard();
        let bytes = units.artifacts()[0].artifact_profile().as_bytes();
        let mut measured = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut measured, &graph);
        Profiles::bind(&units, &mut budget)
            .expect("index")
            .decode(bytes, &mut budget)
            .expect("measure copy");
        let usage = measured.usage();
        // The profile copy is owned once by the original allocation and is
        // never a decode.
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = Profiles::bind(&units, &mut budget)
            .and_then(|profiles| profiles.decode(bytes, &mut budget));
        assert_eq!(
            result.expect("original remainder is owned").as_bytes(),
            bytes
        );
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH
        );
        assert_eq!(budget.usage(), usage);
        // A genuine failure latches the original account.
        budget.refuse();
        assert!(matches!(
            Profiles::bind(&units, &mut budget),
            Err(Error::Refused)
        ));
    }
}
