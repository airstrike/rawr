//! Associate script observations with exact compiler edges from the current graph.

use std::cmp::Ordering;

use super::{Artifact, Script};
use crate::cargo::unit;
use crate::input::stored::{Budget, Error, Result};

/// Recomputed after cold correlation and on decode, never accepted from storage.
pub(in crate::input) fn bind(
    graph: &unit::BuildGraph,
    scripts: &mut [Script],
    artifacts: &[Artifact],
    command_build: &std::path::Path,
    target: &str,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.check()?;
    budget.limited(
        "script association units",
        budget.limits.units,
        graph.scripts().len(),
    )?;
    budget.limited(
        "script association observations",
        budget.limits.units,
        scripts.len(),
    )?;
    budget.storage(graph.scripts().len(), graph.scripts().len())?;
    let mut used = Vec::new();
    used.try_reserve_exact(graph.scripts().len())
        .map_err(|error| budget.failure(Error::Allocation(error)))?;
    used.resize(graph.scripts().len(), false);
    for script in scripts {
        script.compiler = None;
        let is_target = super::platform::target(&script.out_dir, command_build, target, budget)?;
        let mut selected = None;
        // Associations below compare admitted values only: bounded pure work with
        // no decode event; a refused account still stops each step.
        for (run_index, run) in graph.scripts().iter().enumerate() {
            budget.check()?;
            if script.package_id.as_ref() == run.package_id()
                && run.platform() == is_target.then_some(target)
                && selected.replace((run_index, run)).is_some()
            {
                return Err(Error::Encoding(
                    "build-script execution association ambiguous",
                ));
            }
        }
        let (run_index, run) = selected.ok_or(Error::Encoding(
            "build-script execution association missing",
        ))?;
        if std::mem::replace(&mut used[run_index], true) {
            return Err(Error::Encoding("build-script execution association reused"));
        }
        let mut compiler = None;
        for output in graph.artifacts() {
            budget.check()?;
            if run.compiler() == output.semantic_key() && compiler.replace(output).is_some() {
                return Err(Error::Encoding("build-script compiler unit ambiguous"));
            }
        }
        let compiler = compiler.ok_or(Error::Encoding("build-script compiler unit missing"))?;
        let mut observed = None;
        for (index, artifact) in artifacts.iter().enumerate() {
            budget.check()?;
            if compiler.package_id() != artifact.package_id.as_ref() {
                continue;
            }
            if crate::input::compare_artifact(compiler, artifact) == Ordering::Equal
                && observed.replace(index).is_some()
            {
                return Err(Error::Encoding("build-script compiler artifact ambiguous"));
            }
        }
        script.compiler = Some((
            observed.ok_or(Error::Encoding("build-script compiler artifact missing"))?,
            is_target,
        ));
    }
    if used.iter().any(|used| !used) {
        return Err(Error::Encoding(
            "build-script execution association incomplete",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
