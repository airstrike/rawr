//! Ordered owner observations preceding the retained Build artifacts.

use std::path::Path;
use std::slice;

use super::observe::slots::Slots;
use super::view::View;
use super::{observe, Budget, Error, Recipe, Result};
use crate::cargo::unit;
use crate::input::proof::{Entry, Role};
use crate::input::{build, scalar};

/// Append observations from supplied owners, consuming only their exact recipes.
/// The caller establishes current owner association and completes the remaining
/// recipe stream and canonical entry comparison before gaining any authority.
#[cfg(feature = "rustc-driver")]
#[allow(
    clippy::too_many_arguments,
    reason = "the ordered prefix borrows each independently associated owner"
)]
pub(in crate::input) fn require(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    config: &crate::cargo::config::Provenance,
    authority: &[crate::cargo::Input],
    probe: &crate::extract::plan::Capture,
    harness: [&Path; 3],
    toolchain: &crate::cargo::toolchain::Proof,
    internal: &crate::cargo::internal::Recipe,
    target: &str,
    profile: &str,
    build: &build::Retained,
    metadata: &unit::Metadata,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.check()?;
    if toolchain.cargo_commit() != crate::cargo::toolchain::supported_cargo_commit() {
        return Err(Error::Encoding("cargo-commit-skew"));
    }
    let limits = budget.limits;
    scalar::config(
        config.projection(),
        config.digest().as_bytes(),
        &limits,
        budget,
        |value, budget| scalar(recipes, entries, value, budget),
    )?;
    scalar::internal(internal, budget, |value, budget| {
        scalar(recipes, entries, value, budget)
    })?;
    for input in authority {
        if let Some(expected) = input.digest() {
            limited_file(
                recipes,
                entries,
                Role::Authority,
                input.path(),
                expected,
                base,
                "Cargo authority input",
                limits.json_bytes,
                budget,
            )?;
        } else {
            path(recipes, Role::Authority, None, input.path(), true, budget)?;
            entries.reserve(budget)?;
            let entry = observe::limited_optional_file(
                Role::Authority,
                None,
                input.path(),
                base,
                "Cargo authority input",
                limits.json_bytes,
                budget,
            )?;
            if !matches!(entry, Entry::Missing { .. }) {
                return Err(Error::Encoding("absent Cargo authority input appeared"));
            }
            entries.push(entry, budget)?;
        }
    }
    workspace(recipes, entries, authority, metadata, budget)?;
    for (path, digest) in probe.input_artifacts() {
        limited_file(
            recipes,
            entries,
            Role::BuiltOutput,
            path,
            digest,
            base,
            "metadata probe artifact",
            limits.json_bytes,
            budget,
        )?;
    }
    for path in harness {
        file(recipes, entries, Role::Harness, None, path, base, budget)?;
    }
    for (path, digest) in config.input_candidates() {
        optional_file(
            recipes,
            entries,
            path,
            digest,
            base,
            limits.manifest_bytes,
            budget,
        )?;
    }
    scalar::toolchain(toolchain, budget, |value, budget| {
        scalar(recipes, entries, value, budget)
    })?;
    scalar::selection(target, profile, budget, |value, budget| {
        scalar(recipes, entries, value, budget)
    })?;
    packages(recipes, entries, build, metadata, base, budget)
}

fn workspace(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    authority: &[crate::cargo::Input],
    metadata: &unit::Metadata,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let workspace = budget.workspace(authority, metadata)?;
    match recipes.next() {
        Some(Recipe::Workspace { roots }) if roots == &workspace.roots => {}
        _ => {
            return Err(Error::Encoding(
                "workspace roots differ from current owners",
            ))
        }
    }
    entries.push(workspace.entry, budget)
}

#[cfg(test)]
mod workspace_tests {
    use super::*;

    #[test]
    fn current_owner_closure_is_required_even_when_stored_coverage_is_omitted() {
        use crate::cargo::unit::{Metadata, Package, PackageSource};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let metadata = Metadata::fixture_packages([
            Package::fixture(
                "a-harness",
                "harness",
                "0",
                root.join("unobserved-harness"),
                PackageSource::Path {
                    cargo_id: "path+harness".into(),
                },
            ),
            Package::fixture(
                "owner",
                "owner",
                "1",
                root.to_path_buf(),
                PackageSource::Path {
                    cargo_id: "path+owner".into(),
                },
            ),
        ]);
        let limits = crate::cargo::Limits::standard();
        let graph = crate::node::GraphBudget::new();
        let mut allocation = crate::input::Allocation::for_replay(&limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        for recipes in [
            Vec::new(),
            vec![Recipe::Workspace {
                roots: Box::default(),
            }],
        ] {
            let mut entries = Slots::new();
            assert!(matches!(
                workspace(
                    &mut recipes.iter(),
                    &mut entries,
                    &[],
                    &metadata,
                    &mut budget
                ),
                Err(Error::Encoding(
                    "workspace roots differ from current owners"
                ))
            ));
            assert!(entries.as_slice().is_empty());
            assert!(budget.check().is_ok());
        }
        let current = budget.workspace(&[], &metadata).unwrap();
        let expected = current.entry;
        let recipes = [Recipe::Workspace {
            roots: current.roots,
        }];
        let mut entries = Slots::new();
        workspace(
            &mut recipes.iter(),
            &mut entries,
            &[],
            &metadata,
            &mut budget,
        )
        .unwrap();
        assert!(entries.as_slice() == [expected]);
    }
}

fn packages(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    build: &build::Retained,
    metadata: &unit::Metadata,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<()> {
    for (id, retained) in &build.build().evidence.packages {
        budget.check()?;
        let package = metadata.package(id).ok_or(Error::Encoding(
            "owner metadata lacks retained Build package",
        ))?;
        if package.id() != id.as_ref()
            || package.manifest().as_os_str() != retained.manifest.as_os_str()
            || package.root().as_os_str() != retained.root.as_os_str()
        {
            return Err(Error::Encoding(
                "owner metadata and retained Build package differ",
            ));
        }
        file(
            recipes,
            entries,
            Role::PackageManifest,
            Some(id),
            &retained.manifest,
            base,
            budget,
        )?;
        if let Some(path) = &retained.build_script {
            file(
                recipes,
                entries,
                Role::BuildScript,
                Some(id),
                path,
                base,
                budget,
            )?;
        }
        scalar::package(id, &retained.root, budget, |value, budget| {
            scalar(recipes, entries, value, budget)
        })?;
    }
    Ok(())
}

fn scalar(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    value: scalar::Value<'_>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.check()?;
    let recipe = recipes
        .next()
        .ok_or(Error::Encoding("stored prefix recipe is omitted"))?;
    let equal = match (recipe, &value) {
        (
            Recipe::Fact {
                role,
                package_id,
                name,
                value,
            },
            scalar::Value::Fact {
                role: current_role,
                package_id: current_package,
                name: current_name,
                value: current_value,
            },
        ) => {
            (*role, package_id.as_deref(), name.as_ref(), value.as_ref())
                == (
                    *current_role,
                    *current_package,
                    *current_name,
                    *current_value,
                )
        }
        (
            Recipe::EnvironmentProjection { schema, key, value },
            scalar::Value::EnvironmentProjection {
                schema: current_schema,
                key: current_key,
                value: current_value,
            },
        ) => {
            (*schema, key.as_ref(), value.as_deref())
                == (*current_schema, *current_key, *current_value)
        }
        _ => false,
    };
    if !equal {
        return Err(Error::Encoding("stored prefix scalar differs from owner"));
    }
    entries.reserve(budget)?;
    let entry = match value {
        scalar::Value::Fact {
            role,
            package_id,
            name,
            value,
        } => Entry::Fact {
            role,
            package_id: package_id.map(|value| text(value, budget)).transpose()?,
            name: text(name, budget)?,
            value: text(value, budget)?,
        },
        scalar::Value::EnvironmentProjection { schema, key, value } => {
            Entry::EnvironmentProjection {
                schema,
                key: text(key, budget)?,
                value: value.map(|value| text(value, budget)).transpose()?,
            }
        }
    };
    entries.push(entry, budget)
}

fn text(value: &str, budget: &mut Budget<'_>) -> Result<Box<str>> {
    budget.limited(
        "input owned string bytes",
        budget.limits.string_bytes,
        value.len(),
    )?;
    budget.owned_text(value)
}

fn path(
    recipes: &mut slice::Iter<'_, Recipe>,
    role: Role,
    package: Option<&str>,
    path: &Path,
    optional: bool,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.check()?;
    let recipe = recipes
        .next()
        .ok_or(Error::Encoding("stored prefix recipe is omitted"))?;
    let equal = match recipe {
        Recipe::File {
            role: actual_role,
            package_id,
            path: actual,
        } if !optional => {
            *actual_role == role
                && package_id.as_deref() == package
                && actual.as_os_str() == path.as_os_str()
        }
        Recipe::Path {
            role: actual_role,
            package_id,
            path: actual,
        } if optional => {
            *actual_role == role
                && package_id.as_deref() == package
                && actual.as_os_str() == path.as_os_str()
        }
        _ => false,
    };
    if equal {
        Ok(())
    } else {
        Err(Error::Encoding("stored prefix path differs from owner"))
    }
}

fn file(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<()> {
    path(recipes, role, package_id, source, false, budget)?;
    entries.reserve(budget)?;
    let entry = observe::entry::<super::Package>(
        View::File {
            role,
            package_id,
            path: source,
        },
        base,
        budget,
    )?;
    entries.push(entry, budget)
}

/// An artifact-class owner file measured against the configurable `target`,
/// which only records.
#[allow(
    clippy::too_many_arguments,
    reason = "the file joins exact owner identity and its target"
)]
fn limited_file(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    role: Role,
    source: &Path,
    expected: &[u8; 32],
    base: &Path,
    resource: &'static str,
    target: usize,
    budget: &mut Budget<'_>,
) -> Result<()> {
    path(recipes, role, None, source, false, budget)?;
    entries.reserve(budget)?;
    let entry = observe::limited_file(role, None, source, base, resource, target, budget)?;
    if !matches!(&entry, Entry::File { digest, .. } if digest == expected) {
        return Err(Error::Encoding(
            "stored prefix file digest differs from owner",
        ));
    }
    entries.push(entry, budget)
}

/// A Cargo config candidate measured against the configurable manifest
/// `target`.
#[allow(
    clippy::too_many_arguments,
    reason = "the candidate joins exact owner identity and its target"
)]
fn optional_file(
    recipes: &mut slice::Iter<'_, Recipe>,
    entries: &mut Slots<Entry>,
    source: &Path,
    expected: Option<&[u8; 32]>,
    base: &Path,
    target: usize,
    budget: &mut Budget<'_>,
) -> Result<()> {
    path(recipes, Role::Config, None, source, true, budget)?;
    entries.reserve(budget)?;
    let entry = observe::limited_optional_file(
        Role::Config,
        None,
        source,
        base,
        "Cargo config provenance",
        target,
        budget,
    )?;
    let equal = match (&entry, expected) {
        (Entry::Missing { .. }, None) => true,
        (Entry::File { digest, .. }, Some(expected)) => digest == expected,
        _ => false,
    };
    if !equal {
        return Err(Error::Encoding("stored prefix config differs from owner"));
    }
    entries.push(entry, budget)
}

#[cfg(test)]
mod tests;
