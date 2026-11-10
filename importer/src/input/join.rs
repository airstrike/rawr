//! The join from a compiler artifact path to the selected unit that built it.

use std::collections::BTreeMap;
use std::path::Path;

use super::build::{Artifact, Evidence};
use super::{artifact_authority, public_message_key, public_output_key, PublicArtifactKey};
use super::{ArtifactJoin, Result};
use crate::cargo::unit::{BuildGraph, Output};

/// Cargo's artifact messages by output path, and the selected units that
/// reach them by public projection. Both are built once, so each artifact
/// joins by two lookups instead of a walk of every message and unit.
pub(crate) struct Index<'a> {
    messages: BTreeMap<&'a Path, Vec<&'a Artifact>>,
    units: BTreeMap<PublicArtifactKey<'a>, Vec<&'a Output>>,
}

impl<'a> Index<'a> {
    pub(super) fn of(
        evidence: &'a Evidence,
        graph: &'a BuildGraph,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<Self> {
        let mut messages = BTreeMap::<&Path, Vec<&Artifact>>::new();
        for artifact in &evidence.artifacts {
            for output in &artifact.outputs {
                let packages = messages.entry(output.as_path()).or_default();
                // A message naming one output twice is still one message.
                if packages
                    .last()
                    .is_some_and(|last| std::ptr::eq(*last, artifact))
                {
                    continue;
                }
                allocation.items(1)?;
                allocation.reserve(packages, 1)?;
                packages.push(artifact);
            }
        }
        let selected = graph.selected_reachable()?;
        let mut units = BTreeMap::<PublicArtifactKey, Vec<&Output>>::new();
        for unit in graph.artifacts() {
            if !selected.contains(unit.semantic_key()) {
                continue;
            }
            let outputs = units.entry(public_output_key(unit)).or_default();
            allocation.items(1)?;
            allocation.reserve(outputs, 1)?;
            outputs.push(unit);
        }
        Ok(Self { messages, units })
    }

    pub(crate) fn authority(
        &self,
        artifact: &Path,
        allocation: &mut crate::extract::Allocation,
    ) -> Result<ArtifactJoin> {
        let mut authorities = Vec::new();
        for package in self.messages.get(artifact).into_iter().flatten() {
            let units = self.units.get(&public_message_key(package));
            for unit in units.into_iter().flatten() {
                authorities.push((
                    unit.platform(),
                    artifact_authority(unit, package, allocation)?,
                ));
            }
        }
        authorities.sort_unstable_by(|left, right| left.1.semantic_key.cmp(&right.1.semantic_key));
        authorities.dedup_by(|left, right| left == right);
        if authorities.is_empty() {
            return Ok(ArtifactJoin::Missing);
        }
        if authorities.len() > 1 {
            // Cargo's public artifact projection omits the platform, so a host
            // unit and a target unit of one package can share it. Cargo places
            // target-platform output under its triple directory; keep the units
            // whose platform agrees with the artifact's location.
            let under = |triple: &str| {
                artifact
                    .components()
                    .any(|component| component.as_os_str() == triple)
            };
            let triples = authorities
                .iter()
                .filter_map(|(platform, _)| *platform)
                .collect::<Vec<_>>();
            authorities.retain(|(platform, _)| match platform {
                Some(triple) => under(triple),
                None => !triples.iter().any(|triple| under(triple)),
            });
        }
        let Some((_, authority)) = authorities.pop() else {
            return Ok(ArtifactJoin::Ambiguous);
        };
        if !authorities.is_empty() {
            return Ok(ArtifactJoin::Ambiguous);
        }
        Ok(ArtifactJoin::Available(authority))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use super::Index;
    use crate::cargo::unit::{self, reachable};
    use crate::input::build::{Artifact, Evidence, Key};
    use crate::input::{ArtifactJoin, Usage};

    /// A workspace of a few hundred path crates, each depending on the one
    /// before it. Joining every crate's artifact walks the graph once, so the
    /// nodes walked grow with the crates, never with their square: one walk
    /// per join would touch at least 2 × 300 × 300.
    #[test]
    fn joining_every_artifact_of_a_workspace_walks_the_graph_once() {
        const CRATES: usize = 300;
        let mut outputs = Vec::new();
        let mut artifacts = Vec::new();
        for index in 0..CRATES {
            let name = format!("crate{index}");
            let mut output = unit::Output::fixture(
                &format!("path+file:///fixture/{name}#0.1.0"),
                &format!("/fixture/{name}/src/lib.rs"),
                &name,
                "lib",
                "lib",
            );
            if index > 0 {
                let previous: &unit::Output = &outputs[index - 1];
                output = output
                    .with_fixture_dependencies(Box::from([Box::from(previous.semantic_key())]));
            }
            artifacts.push(Artifact {
                package_id: output.package_id().into(),
                outputs: Box::from([PathBuf::from(format!("/fixture/lib{name}.rlib"))]),
                dep_info: PathBuf::from(format!("/fixture/{name}.d")),
                key: Key {
                    package_id: output.package_id().into(),
                    target: output.target().to_path_buf(),
                    target_kinds: output.target_kinds().to_vec().into_boxed_slice(),
                    crate_types: output.crate_types().to_vec().into_boxed_slice(),
                    crate_name: output.crate_name().into(),
                    features: output.features().to_vec().into_boxed_slice(),
                },
                profile: output.artifact_profile().clone(),
                fresh: false,
            });
            outputs.push(output);
        }
        let evidence = Evidence {
            usage: Usage { items: 0, bytes: 0 },
            packages: BTreeMap::new().into(),
            scripts: Box::default(),
            artifacts: artifacts.into_boxed_slice(),
            environment: Box::default(),
        };
        let graph = unit::BuildGraph::fixture_artifacts(outputs.clone().into_boxed_slice());
        let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);

        let before = reachable::walked_nodes();
        let joins = Index::of(&evidence, &graph, &mut allocation).expect("the join index builds");
        for (index, output) in outputs.iter().enumerate() {
            let path = PathBuf::from(format!("/fixture/libcrate{index}.rlib"));
            match joins
                .authority(&path, &mut allocation)
                .expect("the artifact joins")
            {
                ArtifactJoin::Available(authority) => {
                    assert_eq!(authority.semantic_key.as_ref(), output.semantic_key());
                }
                ArtifactJoin::Missing | ArtifactJoin::Ambiguous => {
                    panic!("each crate's artifact joins its one unit")
                }
            }
        }
        let walked = reachable::walked_nodes() - before;
        eprintln!("joined {CRATES} artifacts walking {walked} graph nodes");
        assert!(
            walked <= 6 * CRATES,
            "one walk touches each unit a bounded number of times, not {walked}"
        );
    }
}
