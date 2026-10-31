//! Closure-specific native input witnesses from the normal build's observations.

use serde::Serialize;

use super::{hash, sort, Budget, Entry, Error, Projection, Result, Role, Slots};
use crate::cargo::unit;

/// Exact normal-build units paired with all their transitive input observations.
/// These witnesses cannot be constructed from an API graph or compiler output.
#[derive(Serialize)]
pub(crate) struct Partitions(Box<[Witness]>);

#[derive(Serialize)]
struct Witness {
    unit: Box<str>,
    fingerprint: [u8; 32],
}

impl Partitions {
    pub(crate) fn get(&self, unit: &str) -> Option<[u8; 32]> {
        self.0
            .binary_search_by(|witness| witness.unit.as_ref().cmp(unit))
            .ok()
            .map(|index| self.0[index].fingerprint)
    }
}

impl Projection<'_> {
    /// Project only after every artifact's dep-info has been observed. Unit
    /// closure comes from the same authenticated Cargo graph used by `bind`.
    /// Package-wide observations deliberately retain all host/target executions
    /// of a package; an additional execution can refuse reuse, never erase input.
    pub(in crate::input) fn partitions(
        &self,
        graph: &unit::BuildGraph,
        entries: &[Entry],
        budget: &mut Budget<'_>,
    ) -> Result<Partitions> {
        if self.units.iter().any(|unit| unit.dependency.is_none()) {
            return Err(Error::Encoding(
                "partition dependency observations incomplete",
            ));
        }
        let outputs = graph.artifacts();
        let mut observations = Slots::new();
        let mut context = Slots::new();
        for entry in entries {
            budget.check()?;
            if let Some(package) = package(entry) {
                if !self.evidence.packages.contains_key(package) {
                    return Err(Error::Encoding(
                        "partition observation has no package authority",
                    ));
                }
                observations.push((package, self.entry(entry, budget)?), budget)?;
            } else {
                match entry {
                    // Configuration discovery is revalidated by the enclosing
                    // proof. Present files retain their exact path (including
                    // the authority for relative config values); absent search
                    // locations do not make different empty harnesses unequal.
                    Entry::Missing {
                        role: Role::Config, ..
                    } => {}
                    Entry::File {
                        role: Role::Config, ..
                    }
                    | Entry::Directory {
                        role: Role::Config, ..
                    }
                    | Entry::EnvironmentProjection { .. }
                    | Entry::Fact {
                        role: Role::Toolchain | Role::Target | Role::Profile,
                        ..
                    } => {
                        context.push(self.entry(entry, budget)?, budget)?;
                    }
                    // This aggregate includes absent search paths. Its entire
                    // semantic content is represented by the entries above.
                    Entry::Fact {
                        role: Role::Config,
                        name,
                        ..
                    } if name.as_ref() == "config-provenance" => {}
                    // Cargo manifest/lock and workspace before-images justify
                    // the exact graph and package selection. They are retained
                    // by the consuming proof, not treated as crate source.
                    Entry::Workspace { .. }
                    | Entry::File {
                        role: Role::Authority,
                        ..
                    }
                    | Entry::Missing {
                        role: Role::Authority,
                        ..
                    } => {}
                    Entry::File {
                        role: Role::Harness | Role::BuiltOutput,
                        ..
                    } => {
                        // Require the ordinary projection's exact owner join.
                        // Compiled bytes are products of the witnessed inputs.
                        self.entry(entry, budget)?;
                    }
                    _ => return Err(Error::Encoding("partition observation is unassociated")),
                }
            }
        }
        sort(context.as_mut_slice(), budget, super::compare_digest)?;
        let context = context.finish(budget)?;
        let mut witnesses = Slots::new();
        for root in outputs {
            budget.check()?;
            if root.package_id() == self.harness_package {
                continue;
            }
            let mut closure = Slots::new();
            closure.push(root.semantic_key(), budget)?;
            let mut index = 0;
            while let Some(key) = closure.as_slice().get(index).copied() {
                let (_, dependencies) = graph_unit(graph, key)?;
                for key in dependencies {
                    budget.check()?;
                    let (package, _) = graph_unit(graph, key)?;
                    if package == self.harness_package {
                        return Err(Error::Encoding("partition dependency unit is not exact"));
                    }
                    if !closure.as_slice().contains(&key.as_ref()) {
                        closure.push(key.as_ref(), budget)?;
                    }
                }
                index += 1;
            }
            let closure = closure.finish(budget)?;
            let mut facts = Slots::new();
            // The exact Cargo unit keys bind the full profile, target, feature,
            // edition and edge geometry, including build-script dependencies.
            let mut packages = Slots::new();
            for key in &closure {
                facts.push(hash(&("unit", key), budget)?, budget)?;
                let (owner, _) = graph_unit(graph, key)?;
                if !observations
                    .as_slice()
                    .iter()
                    .any(|(package, _)| *package == owner)
                {
                    return Err(Error::Encoding("partition package observations missing"));
                }
                if !packages.as_slice().contains(&owner) {
                    packages.push(owner, budget)?;
                }
            }
            for (package, observation) in observations.as_slice() {
                if packages.as_slice().contains(package) {
                    facts.push(*observation, budget)?;
                }
            }
            sort(facts.as_mut_slice(), budget, super::compare_digest)?;
            let facts = facts.finish(budget)?;
            witnesses.push(
                Witness {
                    unit: budget.owned_text(root.semantic_key())?,
                    fingerprint: hash(
                        &("rawr.input.native.partition/v1", &context, facts),
                        budget,
                    )?,
                },
                budget,
            )?;
        }
        sort(witnesses.as_mut_slice(), budget, |left, right, budget| {
            budget.check()?;
            Ok(left.unit.cmp(&right.unit))
        })?;
        if witnesses
            .as_slice()
            .windows(2)
            .any(|pair| pair[0].unit == pair[1].unit)
        {
            return Err(Error::Encoding("partition unit witness is duplicated"));
        }
        Ok(Partitions(witnesses.finish(budget)?))
    }
}

fn graph_unit<'graph>(
    graph: &'graph unit::BuildGraph,
    key: &str,
) -> Result<(&'graph str, &'graph [std::sync::Arc<str>])> {
    let mut found = graph
        .artifacts()
        .iter()
        .filter(|output| output.semantic_key() == key)
        .map(|output| (output.package_id(), output.dependency_keys()))
        .chain(
            graph
                .scripts()
                .iter()
                .filter(|script| script.semantic_key() == key)
                .map(|script| (script.package_id(), script.dependency_keys())),
        );
    let unit = found
        .next()
        .ok_or(Error::Encoding("partition dependency unit missing"))?;
    if found.next().is_some() {
        return Err(Error::Encoding("partition dependency unit duplicated"));
    }
    Ok(unit)
}

fn package(entry: &Entry) -> Option<&str> {
    match entry {
        Entry::File { package_id, .. }
        | Entry::Directory { package_id, .. }
        | Entry::Missing { package_id, .. }
        | Entry::Fact { package_id, .. } => package_id.as_deref(),
        Entry::Registry { package_id, .. }
        | Entry::Git { package_id, .. }
        | Entry::Environment { package_id, .. }
        | Entry::BuildMessage { package_id, .. }
        | Entry::Directive { package_id, .. } => Some(package_id),
        Entry::Workspace { .. } | Entry::EnvironmentProjection { .. } => None,
    }
}
