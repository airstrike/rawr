//! Dep-info data joined to retained Build observations under the owner accounts.

use std::path::{Path, PathBuf};

use super::{scan, Dependency};
use crate::input::build;
use crate::input::stored::{self, observe, view, Budget};

pub(in crate::input) mod index;
mod token;

/// Observe one associated dep-info recipe without acquiring current authority.
pub(in crate::input) fn read(
    view: view::View<'_, stored::Package>,
    build: &build::Retained,
    base: &Path,
    budget: &mut Budget<'_>,
) -> stored::Result<Dependency> {
    acquire(view, build, base, budget, |_| Ok(()))
}

fn acquire(
    view: view::View<'_, stored::Package>,
    build: &build::Retained,
    base: &Path,
    budget: &mut Budget<'_>,
    before_open: impl FnOnce(&Path) -> stored::Result<()>,
) -> stored::Result<Dependency> {
    budget.check()?;
    let view::View::DepInfo {
        package_id,
        path,
        package_root,
        out_directories,
    } = view
    else {
        return Err(policy("recipe is not a dep-info observation"));
    };
    let evidence = &build.build().evidence;
    let package = evidence
        .packages
        .get(package_id)
        .ok_or_else(|| policy("dep-info Build package is not associated"))?;
    if package.root.as_os_str() != package_root.as_os_str()
        || !evidence.artifacts.iter().any(|artifact| {
            artifact.package_id.as_ref() == package_id
                && artifact.dep_info.as_os_str() == path.as_os_str()
        })
        || !out_directories
            .iter()
            .map(|path| path.as_os_str())
            .eq(evidence
                .scripts
                .iter()
                .filter(|script| script.package_id.as_ref() == package_id)
                .map(|script| script.out_dir.as_os_str()))
    {
        return Err(policy("dep-info Build execution is not associated"));
    }
    observe::path::metadata(path, false, budget)?;
    before_open(path)?;
    let bytes = observe::read::dep_info(path, budget)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| policy("dep-info-utf8"))?;
    scan::complete(text).map_err(syntax)?;
    let environment = environment(text, budget)?;
    let tokens = scan::Tokens::raw(text).map_err(syntax)?;
    let mut token = token::Token::new();
    let mut sources = observe::slots::Slots::new();
    let mut index = index::Index::new();
    for event in tokens {
        match event.map_err(syntax)? {
            scan::Event::Character(character) => token.push(character, budget)?,
            scan::Event::Boundary => {
                source(
                    &mut sources,
                    &mut index,
                    token.as_str(),
                    package_root,
                    base,
                    budget,
                )?;
                token.clear();
            }
        }
    }
    if sources.as_slice().is_empty() {
        return Err(policy("missing-dep-info-source"));
    }
    drop(index);
    let sources = sources.sorted(budget)?;
    for (key, value) in &environment {
        if key.as_ref() == "OUT_DIR" {
            let value = value
                .as_deref()
                .ok_or_else(|| policy("unproved-out-dir-env-dep"))?;
            let current = observe::path::canonical(Path::new(value), base, true, budget)?;
            if !out_directories
                .iter()
                .any(|candidate| candidate == &current)
            {
                return Err(policy("unproved-out-dir-env-dep"));
            }
        }
    }
    Ok(Dependency {
        sources,
        environment,
    })
}

#[allow(
    clippy::type_complexity,
    reason = "the shared dep-info representation retains exact optional environment values"
)]
fn environment(
    text: &str,
    budget: &mut Budget<'_>,
) -> stored::Result<Box<[(Box<str>, Option<Box<str>>)]>> {
    let mut values = observe::slots::Slots::<(Box<str>, Option<Box<str>>)>::new();
    let mut index = index::Index::new();
    for record in scan::Environment::read(text) {
        // Key and value lengths only record.
        let record = record.map_err(|error| match error {
            scan::Error::Malformed(_) => policy("malformed-env-dep"),
        })?;
        let limit = budget.limits.string_bytes;
        budget.limited("rustc env-dep key", limit, record.key.len())?;
        if let Some(value) = record.value {
            budget.limited("rustc env-dep value", limit, value.len())?;
        }
        let key = budget.owned_text(record.key)?;
        let value = record
            .value
            .map(|value| budget.owned_text(value))
            .transpose()?;
        let vacant = match index.locate(values.as_slice(), |(key, _)| key.as_ref().cmp(record.key))
        {
            Err(_) => return Err(policy("duplicate-env-dep")),
            Ok(vacant) => vacant,
        };
        values.push((key, value), budget)?;
        index.insert(vacant, budget)?;
    }
    drop(index);
    values.sorted(budget)
}

fn source(
    sources: &mut observe::slots::Slots<PathBuf>,
    index: &mut index::Index,
    token: &str,
    package_root: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> stored::Result<()> {
    budget.limited(
        "dep-info source paths",
        budget.limits.items,
        sources.as_slice().len().saturating_add(1),
    )?;
    let raw = Path::new(token);
    let candidate = if raw.is_absolute() {
        let length = raw.as_os_str().len();
        budget.storage(1, length)?;
        let mut candidate = PathBuf::new();
        candidate
            .try_reserve_exact(length)
            .map_err(|error| budget.failure(stored::Error::Allocation(error)))?;
        candidate.push(raw);
        candidate
    } else {
        observe::path::joined(package_root, raw, budget)?
    };
    match observe::path::metadata(&candidate, false, budget) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) | Err(stored::Error::Observation(observe::Error::Io { .. })) => {
            return Err(policy("missing-dep-info-source"));
        }
        Err(error) => return Err(error),
    }
    let canonical = observe::path::canonical(&candidate, base, false, budget)?;
    if let Ok(vacant) = index.locate(sources.as_slice(), |value| value.cmp(&canonical)) {
        sources.push(canonical, budget)?;
        index.insert(vacant, budget)?;
    }
    Ok(())
}

fn syntax(error: scan::Syntax) -> stored::Error {
    policy(match error {
        scan::Syntax::TrailingLineEscape => "trailing line escape",
        scan::Syntax::MissingRule => "malformed-dep-info",
        scan::Syntax::TrailingEscape => "trailing escape",
    })
}

fn policy(reason: &'static str) -> stored::Error {
    observe::Error::Policy(reason).into()
}

#[cfg(test)]
mod tests;
