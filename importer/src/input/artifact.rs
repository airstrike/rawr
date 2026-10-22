//! Borrowed recipes derived from one already-parsed artifact dep-info record.

use std::path::Path;

use super::scalar::account::Account;
use super::{dependency, proof, Allocation};
use crate::Result;

/// Derived data only; the caller retains artifact and package association.
pub(super) enum Value<'value> {
    File {
        package_id: &'value str,
        path: &'value Path,
    },
    Environment {
        package_id: &'value str,
        execution: &'value str,
        key: &'value str,
        value: Option<&'value str>,
    },
}

impl Value<'_> {
    /// The cold caller admits its recipe slot before retaining these fields.
    pub(super) fn retain(self, allocation: &mut Allocation) -> Result<proof::Recipe> {
        match self {
            Self::File { package_id, path } => proof::Recipe::package_file(
                proof::Role::DependencySource,
                package_id,
                path,
                allocation,
            ),
            Self::Environment {
                package_id,
                execution,
                key,
                value,
            } => proof::Recipe::environment(package_id, execution, key, value, allocation),
        }
    }
}

/// Emit sources before admitting and rendering the dep-info execution path,
/// then emit its ordered environment. The DepInfo header and read precede this.
pub(super) fn derived<A: Account>(
    dependency: &dependency::Dependency,
    package_id: &str,
    dep_info: &Path,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    for source in &dependency.sources {
        emit(
            Value::File {
                package_id,
                path: source,
            },
            allocation,
        )?;
    }
    // Cold capture retains its existing path charge here, even for an empty
    // environment. Current comparison borrows the path through the same account.
    allocation.package_path(dep_info)?;
    let execution = allocation.path_text("rustc dep-info execution", dep_info)?;
    for (key, value) in &dependency.environment {
        emit(
            Value::Environment {
                package_id,
                execution,
                key,
                value: value.as_deref(),
            },
            allocation,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
