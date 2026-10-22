//! Ordered Build-section observations, without a complete input proof.

use std::path::Path;
use std::slice;

use super::observe::slots::Slots;
use super::{Budget, Error, Package, Recipe, Result};
use crate::cargo::unit;
use crate::input::proof::Entry;
use crate::input::{artifact, build, declaration, dependency, package, replay, script};

mod codec;
mod compare;
mod entry;

/// Consume the final recipe section after the caller has checked the complete
/// prefix through this same cursor. Every artifact and script contributes its
/// observations to the caller's original buffer. Success only covers this
/// section: the caller must associate the retained owners and compare the full
/// sorted, deduplicated entry set before constructing any current proof.
#[allow(
    clippy::too_many_arguments,
    reason = "the complete owner supplies one cursor, buffer and original account"
)]
pub(in crate::input) fn require(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    retained: &build::Retained,
    cargo_env_overrides: bool,
    metadata: &unit::Metadata,
    cargo_home: Option<&Path>,
    base: &Path,
    mut observe: impl FnMut(usize, &dependency::Dependency, &mut Budget<'_>) -> Result<()>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.check()?;
    let build = retained.build();
    if !build.package_files.is_empty() {
        return Err(Error::Encoding(
            "retained Build still owns package file sets",
        ));
    }
    for (index, artifact) in build.evidence.artifacts.iter().enumerate() {
        let package = build
            .evidence
            .packages
            .get(&artifact.package_id)
            .ok_or(Error::Encoding("artifact Build package is absent"))?;
        // The cold producer retains this header before reading dep-info. Do
        // not search for a later header or acquire before comparing this one.
        let recipe = next(recipes, budget)?;
        compare::header(recipe, artifact, package, &build.evidence.scripts)?;
        let dependency = dependency::observed::read(recipe.view(), retained, base, budget)?;
        observe(index, &dependency, budget)?;
        push(entries, budget, |budget| {
            replay::observed::dep_info(
                &dependency,
                &artifact.package_id,
                &artifact.dep_info,
                budget,
            )
        })?;
        artifact::derived(
            &dependency,
            &artifact.package_id,
            &artifact.dep_info,
            budget,
            |value, budget| {
                compare::artifact(&value, next(recipes, budget)?)?;
                push(entries, budget, |budget| {
                    entry::artifact(value, base, budget)
                })
            },
        )?;
    }
    let mut fallback = Slots::new();
    for script in &build.evidence.scripts {
        let package = build
            .evidence
            .packages
            .get(&script.package_id)
            .ok_or(Error::Encoding("script Build package is absent"))?;
        // Saved output is acquired once, at the cold producer's pre-header
        // stage. The parsed data serves both the structured entry and coverage.
        let declarations = declaration::observed::read(
            &script.package_id,
            &script.out_dir,
            retained,
            base,
            budget,
        )?;
        let execution = script::declarations(
            script,
            package,
            &declarations,
            cargo_env_overrides,
            &build.evidence.environment,
            budget,
            |value, budget| {
                compare::script(&value, next(recipes, budget)?)?;
                push(entries, budget, |budget| {
                    entry::script(value, &declarations, base, budget)
                })
            },
        )?;
        if script::fallback(&declarations)
            && !fallback.as_slice().contains(&script.package_id.as_ref())
        {
            budget.limited(
                "default package file sets",
                budget.limits.packages,
                fallback.as_slice().len().saturating_add(1),
            )?;
            fallback.push(script.package_id.as_ref(), budget)?;
            let current = metadata
                .package(&script.package_id)
                .ok_or(Error::Encoding("fallback metadata package is absent"))?;
            if current.id() != script.package_id.as_ref()
                || current.root().as_os_str() != package.root.as_os_str()
                || current.manifest().as_os_str() != package.manifest.as_os_str()
            {
                return Err(Error::Encoding(
                    "fallback metadata and Build package differ",
                ));
            }
            match current.source() {
                unit::PackageSource::Path { .. } | unit::PackageSource::Git { .. } => {
                    return Err(Error::IncompleteWitness);
                }
                unit::PackageSource::Registry { .. } => {}
            }
            let Recipe::Package(Package::Registry(registry)) = next(recipes, budget)? else {
                return Err(Error::Encoding("stored fallback recipe differs"));
            };
            let cargo_home = cargo_home.ok_or(Error::Encoding("missing-cargo-home-authority"))?;
            push(entries, budget, |budget| {
                package::observed::registry(registry, current, cargo_home, base, budget)
            })?;
        }
        script::outputs(script, execution, budget, |value, budget| {
            compare::script(&value, next(recipes, budget)?)?;
            push(entries, budget, |budget| {
                entry::script(value, &declarations, base, budget)
            })
        })?;
    }
    budget.check()?;
    if !recipes.as_slice().is_empty() {
        return Err(Error::Encoding("stored Build recipes remain"));
    }
    Ok(())
}

fn next<'recipe>(
    recipes: &mut slice::Iter<'recipe, Recipe>,
    budget: &mut Budget<'_>,
) -> Result<&'recipe Recipe> {
    budget.check()?;
    recipes
        .next()
        .ok_or(Error::Encoding("stored Build recipe is omitted"))
}

fn push(
    entries: &mut Slots<Entry>,
    budget: &mut Budget<'_>,
    observe: impl FnOnce(&mut Budget<'_>) -> Result<Entry>,
) -> Result<()> {
    budget.limited(
        "input proof entries",
        budget.limits.items,
        entries.as_slice().len().saturating_add(1),
    )?;
    // Admit the real entry slot before its fields or filesystem observation.
    entries.reserve(budget)?;
    let entry = observe(budget)?;
    entries.push(entry, budget)
}

#[cfg(test)]
mod tests;
