//! Cargo's artifact messages and a graph's units, each ordered by its public
//! projection, so a unit finds the messages it matches, and a message the
//! units it matches, by a binary search instead of a walk of every one
//! (#1846).

use super::build::Artifact;
use super::{public_message_key, public_output_key};
use crate::cargo::unit::Output;

/// Cargo's artifact messages in the order of their public projection; equal
/// projections keep the order Cargo reported them in.
pub(super) struct Messages<'a>(Vec<&'a Artifact>);

impl<'a> Messages<'a> {
    pub(super) fn of(artifacts: &'a [Artifact]) -> Self {
        let mut ordered: Vec<&Artifact> = artifacts.iter().collect();
        ordered.sort_by(|left, right| public_message_key(left).cmp(&public_message_key(right)));
        Self(ordered)
    }

    /// The messages whose public projection is `output`'s, in Cargo's order.
    pub(super) fn matching(&self, output: &Output) -> &[&'a Artifact] {
        let key = public_output_key(output);
        let start = self
            .0
            .partition_point(|artifact| public_message_key(artifact) < key);
        let end = self
            .0
            .partition_point(|artifact| public_message_key(artifact) <= key);
        &self.0[start..end]
    }
}

/// A graph's units in the order of their public projection.
pub(super) struct Units<'a>(Vec<&'a Output>);

impl<'a> Units<'a> {
    pub(super) fn of(outputs: &'a [Output]) -> Self {
        let mut ordered: Vec<&Output> = outputs.iter().collect();
        ordered.sort_by(|left, right| public_output_key(left).cmp(&public_output_key(right)));
        Self(ordered)
    }

    /// Whether some unit's public projection is `artifact`'s.
    pub(super) fn contains(&self, artifact: &Artifact) -> bool {
        let key = public_message_key(artifact);
        self.0
            .binary_search_by(|output| public_output_key(output).cmp(&key))
            .is_ok()
    }
}
