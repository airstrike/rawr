//! Saved-output data reobserved under the current owner's original accounts.

use std::path::Path;

use super::{scan, Declarations};
use crate::input::build;
use crate::input::stored::{self, observe, Budget};

#[derive(Debug)]
enum Failure<'bytes> {
    Grammar(scan::Error<'bytes>),
    Account(stored::Error),
}

impl<'bytes> From<scan::Error<'bytes>> for Failure<'bytes> {
    fn from(error: scan::Error<'bytes>) -> Self {
        Self::Grammar(error)
    }
}

impl From<stored::Error> for Failure<'_> {
    fn from(error: stored::Error) -> Self {
        Self::Account(error)
    }
}

/// Bind to an actual retained Build execution before consulting the filesystem.
/// The returned declarations are observations, not a current owner receipt.
pub(in crate::input) fn read(
    package_id: &str,
    out_dir: &Path,
    build: &build::Retained,
    base: &Path,
    budget: &mut Budget<'_>,
) -> stored::Result<Declarations> {
    acquire(package_id, out_dir, build, base, budget, |_| Ok(()))
}

fn acquire(
    package_id: &str,
    out_dir: &Path,
    build: &build::Retained,
    base: &Path,
    budget: &mut Budget<'_>,
    before_open: impl FnOnce(&Path) -> stored::Result<()>,
) -> stored::Result<Declarations> {
    budget.check()?;
    let evidence = &build.build().evidence;
    if !evidence.packages.contains_key(package_id)
        || !evidence.scripts.iter().any(|script| {
            script.package_id.as_ref() == package_id
                && script.out_dir.as_os_str() == out_dir.as_os_str()
        })
    {
        return Err(
            observe::Error::Policy("saved-output Build execution is not associated").into(),
        );
    }
    budget.limited(
        "saved-output PackageId",
        budget.limits.string_bytes,
        package_id.len(),
    )?;
    let package_id = budget.owned_text(package_id)?;
    let out_dir = observe::path::canonical(out_dir, base, true, budget)?;
    if out_dir.file_name().and_then(|name| name.to_str()) != Some(super::OUT_DIRECTORY)
        || out_dir
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(super::BUILD_DIRECTORY)
    {
        return Err(observe::Error::Policy("saved-output-layout").into());
    }
    let parent = out_dir
        .parent()
        .ok_or(observe::Error::Policy("saved-output-layout"))?;
    let candidate = observe::path::joined(parent, Path::new(super::OUTPUT_FILE), budget)?;
    let output = observe::path::canonical(&candidate, base, false, budget)?;
    let metadata = observe::path::metadata(&output, false, budget)?;
    let maximum = usize::try_from(metadata.len()).map_err(|_| budget.overflow())?;
    // Saved build output's JSON accounting target only records.
    budget.limited(
        "Cargo saved build output",
        budget.limits.json_bytes,
        maximum,
    )?;
    before_open(&output)?;
    let bytes = observe::read::bounded(&output, maximum, budget)?;
    let mut paths = observe::slots::Slots::new();
    let mut environment = observe::slots::Slots::new();
    let mut directives = observe::slots::Slots::new();
    let limits = budget.limits;
    let scanned = scan::visit(&bytes, |event| -> Result<(), Failure<'_>> {
        let (values, text) = match event {
            scan::Event::Directive(text) => (&mut directives, text),
            scan::Event::Path(text) => (&mut paths, text),
            scan::Event::Environment(text) => (&mut environment, text),
        };
        budget.limited("input owned string bytes", limits.string_bytes, text.len())?;
        let value = budget.owned_text(text)?;
        values.push(value, budget)?;
        Ok(())
    });
    match scanned {
        Ok(()) => {}
        Err(Failure::Account(error)) => return Err(error),
        Err(Failure::Grammar(scan::Error::Truncated)) => {
            return Err(observe::Error::Policy("truncated-saved-output").into());
        }
        Err(Failure::Grammar(scan::Error::Utf8(_))) => {
            return Err(observe::Error::Policy("saved-output-utf8").into());
        }
        Err(Failure::Grammar(scan::Error::Malformed(_))) => {
            return Err(observe::Error::Policy("malformed-saved-output").into());
        }
    }
    Ok(Declarations {
        package_id,
        output,
        paths: paths.finish(budget)?,
        environment: environment.finish(budget)?,
        directives: directives.finish(budget)?,
    })
}

#[cfg(test)]
mod tests;
