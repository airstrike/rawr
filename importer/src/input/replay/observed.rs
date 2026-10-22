//! Structured entry data formatted from already acquired observations.

use std::path::Path;

use super::{canonical, Dependency, SavedOutput};
use crate::input::declaration::Declarations;
use crate::input::dependency;
use crate::input::proof::{Entry, Role};
use crate::input::scalar::account;
use crate::input::stored::{Budget, Error, Result};

/// Format already-associated dep-info data without rereading its source. The
/// supplied execution spelling remains part of this ordinary fact's name.
pub(in crate::input) fn dep_info(
    dependency: &dependency::Dependency,
    package_id: &str,
    execution_path: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    let digest = dependency_digest(dependency, budget)?;
    let execution = path_text(execution_path, budget)?;
    let name = account::formatted(budget, format_args!("{package_id}:{execution}"))?;
    if name.capacity() != name.len() {
        budget.storage(0, name.len())?;
    }
    let name = name.into_boxed_str();
    let package_id = Some(scalar(package_id, budget)?);
    let value = hex(digest, budget)?;
    Ok(Entry::Fact {
        role: Role::DepInfo,
        package_id,
        name,
        value,
    })
}

/// Hash the same structured exact evidence for its separate native association.
pub(in crate::input) fn dependency_digest(
    dependency: &dependency::Dependency,
    budget: &mut Budget<'_>,
) -> Result<[u8; 32]> {
    budget.check()?;
    // Serde's PathBuf implementation emits the same borrowed UTF-8 strings as
    // the legacy converted sources. Validate in source order before hashing.
    for source in &dependency.sources {
        path_text(source, budget)?;
    }
    canonical::digest(
        &Dependency {
            sources: &dependency.sources,
            environment: &dependency.environment,
        },
        "structured replay bytes",
        budget.limits.json_bytes,
        budget,
    )
}

/// Preserve the supplied execution spelling and the declarations' exact source.
/// No file is reread and this ordinary entry supplies no current-proof authority.
pub(in crate::input) fn saved_output(
    declarations: &Declarations,
    execution: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    budget.check()?;
    let digest = canonical::digest(
        &SavedOutput::borrow(declarations),
        "structured replay bytes",
        budget.limits.json_bytes,
        budget,
    )?;
    let package_id = scalar(&declarations.package_id, budget)?;
    let execution = path(execution, budget)?;
    let name = scalar("saved-output", budget)?;
    let source = Some(path(&declarations.output, budget)?);
    let value = hex(digest, budget)?;
    Ok(Entry::BuildMessage {
        package_id,
        execution,
        name,
        source,
        value,
    })
}

fn hex(digest: [u8; 32], budget: &mut Budget<'_>) -> Result<Box<str>> {
    let mut encoded = [0_u8; 64];
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (byte, pair) in digest.iter().zip(encoded.as_chunks_mut::<2>().0.iter_mut()) {
        pair[0] = DIGITS[usize::from(byte >> 4)];
        pair[1] = DIGITS[usize::from(byte & 0x0f)];
    }
    let encoded = std::str::from_utf8(&encoded)
        .map_err(|_| Error::Encoding("structured digest encoding changed"))?;
    budget.owned_text(encoded)
}

fn scalar(value: &str, budget: &mut Budget<'_>) -> Result<Box<str>> {
    budget.limited(
        "input owned string bytes",
        budget.limits.string_bytes,
        value.len(),
    )?;
    budget.owned_text(value)
}

fn path(value: &Path, budget: &mut Budget<'_>) -> Result<Box<str>> {
    let value = path_text(value, budget)?;
    budget.owned_text(value)
}

fn path_text<'path>(value: &'path Path, budget: &mut Budget<'_>) -> Result<&'path str> {
    let value = value
        .to_str()
        .ok_or(Error::Encoding("structured replay path is not Unicode"))?;
    budget.limited("input path bytes", crate::input::LINE_BYTES, value.len())?;
    Ok(value)
}

#[cfg(test)]
mod tests;
