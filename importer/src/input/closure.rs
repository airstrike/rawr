//! Each unit's dependencies and the canonical paths of its artifacts, built
//! once per graph, so the closure of every extracted crate is a walk of this
//! index: no unit or message is scanned and no path resolved again (#1846).

use std::collections::BTreeMap;

use super::build::Artifact;
use super::messages::Messages;
use crate::cargo::unit::BuildGraph;
use crate::cargo::Digest;

/// What a unit's closure needs of one semantic unit.
#[derive(Default)]
struct Unit {
    /// The semantic keys of its dependencies, as its outputs name them.
    dependencies: Vec<Box<str>>,
    /// The canonical path of each output of each message its outputs
    /// match, once per match, as a closure collects them.
    paths: Vec<Box<str>>,
}

/// The closure index of the graph whose content is `graph`.
pub(super) struct Index {
    graph: Digest,
    units: BTreeMap<Box<str>, Unit>,
}

impl Index {
    pub(super) fn of(graph: &BuildGraph, artifacts: &[Artifact]) -> Self {
        let messages = Messages::of(artifacts);
        // A message matched by many units is resolved once.
        let mut canonical = BTreeMap::<*const Artifact, Vec<Box<str>>>::new();
        let mut units = BTreeMap::<Box<str>, Unit>::new();
        for output in graph.artifacts() {
            let unit = units.entry(output.semantic_key().into()).or_default();
            unit.dependencies
                .extend(output.dependency_keys().iter().map(|key| Box::from(&**key)));
            for artifact in messages.matching(output) {
                let paths = canonical
                    .entry(std::ptr::from_ref(*artifact))
                    .or_insert_with(|| {
                        artifact
                            .outputs
                            .iter()
                            .filter_map(|path| {
                                let path =
                                    std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
                                path.to_str().map(Box::from)
                            })
                            .collect()
                    });
                unit.paths.extend(paths.iter().cloned());
            }
        }
        Self {
            graph: graph.digest(),
            units,
        }
    }

    /// Whether this index is of `graph`'s content.
    pub(super) fn is_of(&self, graph: &BuildGraph) -> bool {
        self.graph == graph.digest()
    }

    /// The canonical artifact paths of every unit `unit_key` reaches
    /// through its dependencies, its own excluded, each once per match and
    /// in no order: the caller sorts and charges them.
    pub(super) fn closure<'a>(&'a self, unit_key: &'a str) -> Vec<&'a str> {
        let mut seen = std::collections::BTreeSet::from([unit_key]);
        let mut queue = vec![unit_key];
        let mut paths = Vec::new();
        while let Some(key) = queue.pop() {
            let Some(unit) = self.units.get(key) else {
                continue;
            };
            for dependency in &unit.dependencies {
                if seen.insert(dependency) {
                    queue.push(dependency);
                }
            }
            if key != unit_key {
                paths.extend(unit.paths.iter().map(AsRef::as_ref));
            }
        }
        paths
    }
}
