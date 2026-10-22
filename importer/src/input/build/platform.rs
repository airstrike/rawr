//! Pinned Cargo host/target placement within the authenticated command geometry.

use std::path::Path;

use crate::input::stored::{Budget, Error, Result};

/// Interpret only a path already associated with a current public Build record.
pub(in crate::input) fn target(
    path: &Path,
    command_build: &Path,
    target: &str,
    budget: &mut Budget<'_>,
) -> Result<bool> {
    // A comparison over admitted paths: bounded pure work with no decode
    // event; a refused account still stops it here.
    budget.check()?;
    let relative = path
        .strip_prefix(command_build)
        .map_err(|_| Error::Encoding("Build output outside command build directory"))?;
    Ok(relative
        .components()
        .next()
        .is_some_and(|part| part.as_os_str() == target))
}
