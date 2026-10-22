use crate::cargo::universe::{MetadataProof, ProtectedPackage};
use crate::input::build::Evidence;
use crate::input::stored::observe::slots::Slots;
use crate::input::stored::{Budget, Error, Result};

use super::text;

pub(in crate::input) fn token(
    package: &ProtectedPackage,
    budget: &mut Budget<'_>,
) -> Result<[u8; 64]> {
    budget.check()?;
    let bytes = package
        .token_hash_bytes()
        .ok_or_else(|| budget.overflow())?;
    budget.hash(bytes)?;
    Ok(package.token_bytes())
}

fn token_text(token: &[u8; 64]) -> Result<&str> {
    std::str::from_utf8(token).map_err(|_| Error::Encoding("protected token encoding differs"))
}

/// Preserve the cold projector's ordered literal replacements, including every
/// intermediate value; a root or output absent from the current value would
/// replace nothing, so it costs no token, format or copy. Borrowed indexes own
/// only their admitted slot layouts.
pub(in crate::input) fn value(
    value: &str,
    evidence: &Evidence,
    protected: &MetadataProof,
    budget: &mut Budget<'_>,
) -> Result<String> {
    budget.check()?;
    let mut roots = Slots::new();
    for package in protected.normalization_roots() {
        roots.push(package, budget)?;
    }
    ProtectedPackage::sort_roots(roots.as_mut_slice());
    let mut normalized = text::format(format_args!("{value}"), budget)?;
    for package in roots.as_slice() {
        let Some(root) = package.root().to_str() else {
            continue;
        };
        if !normalized.contains(root) {
            continue;
        }
        let token = token(package, budget)?;
        let replacement = text::format(format_args!("$PACKAGE/{}", token_text(&token)?), budget)?;
        normalized = text::replace(&normalized, root, &replacement, budget)?;
    }
    let mut outputs = Slots::new();
    for script in &evidence.scripts {
        let Some(path) = script.out_dir.to_str() else {
            continue;
        };
        let Some(package) = protected.package(&script.package_id) else {
            continue;
        };
        // The cold path computes these tokens before sorting its output index.
        outputs.reserve(budget)?;
        let token = token(package, budget)?;
        let token = text::format(format_args!("{}", token_text(&token)?), budget)?;
        outputs.push((path, token), budget)?;
    }
    outputs
        .as_mut_slice()
        .sort_unstable_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    for (path, token) in outputs.as_slice() {
        if !normalized.contains(path) {
            continue;
        }
        let replacement = text::format(format_args!("$OUT/{token}"), budget)?;
        normalized = text::replace(&normalized, path, &replacement, budget)?;
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests;
