// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! One normalized target group; no completed manifest or current authority.

use super::super::{
    super::{
        schema::{manifest::Targets, Target},
        session::Session,
        storage, Error, Step,
    },
    path, Warnings,
};
use super::{files::Text, infer, merge, resolve, target, validate};
use crate::input::membership::host;
use std::path::{Path, PathBuf};

pub(in super::super) enum Input<'a, 'd, 'scratch> {
    Declared(Option<&'a Targets<'d>>),
    Embedded {
        name: &'scratch str,
        path: &'scratch Path,
    },
}
pub(in super::super) struct Normalized<'a, 'd> {
    original: Option<&'a Target<'d>>,
    name: Text<'d>,
    path: PathBuf,
}
impl<'a, 'd> Normalized<'a, 'd> {
    pub(in super::super) fn original(&self) -> Option<&'a Target<'d>> {
        self.original
    }
    pub(in super::super) fn name(&self) -> &str {
        self.name.as_str()
    }
    pub(in super::super) fn path(&self) -> &Path {
        &self.path
    }
}

#[allow(clippy::too_many_arguments)]
pub(in super::super) fn group<'a, 'd>(
    original: Input<'a, 'd, '_>,
    kind: resolve::Kind,
    root: &Path,
    package_name: &str,
    edition: u16,
    autodiscover: Option<bool>,
    has_lib: bool,
    warnings: &mut Warnings,
    errors: &mut validate::Errors,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<storage::Buffer<Normalized<'a, 'd>>, String>> {
    session.check()?;
    let (fast, original, mut declared) = match original {
        Input::Declared(original) => {
            let fast = merge::are_normalized(original, autodiscover, session)?;
            (fast, original, None)
        }
        Input::Embedded {
            name,
            path: explicit,
        } => {
            if !matches!(kind, resolve::Kind::Bin) {
                return session.fail(Error::State);
            }
            let name = target::owned(&[name], false, session)?;
            let path = path::join(Path::new(""), explicit, session)?;
            let mut declared = storage::Buffer::new();
            declared.push(
                merge::Draft {
                    original: None,
                    name: Some(Text::Owned(name)),
                    path: Some(path),
                },
                session,
            )?;
            (autodiscover == Some(false), None, Some(declared))
        }
    };
    let mut output = storage::Buffer::new();
    if fast {
        if declared.is_none() {
            declared = original
                .map(|original| merge::declared(Some(original), session))
                .transpose()?;
        }
        for draft in declared.into_iter().flat_map(storage::Buffer::into_iter) {
            check(&draft, kind, edition, warnings, errors, session)?;
            let Some(explicit) = draft.path.as_deref() else {
                return session.fail(Error::State);
            };
            let path = path::normalize(explicit, session)?;
            push(draft, path, &mut output, session)?;
        }
        return Ok(Ok(output));
    }
    let (human, label, flag, directory) = match kind {
        resolve::Kind::Bin => ("binary", "bin", "autobins", "src/bin"),
        resolve::Kind::Example => ("example", "example", "autoexamples", "examples"),
        resolve::Kind::Test => ("test", "test", "autotests", "tests"),
        resolve::Kind::Bench => ("benchmark", "bench", "autobenches", "benches"),
    };
    let inferred = if matches!(kind, resolve::Kind::Bin) {
        infer::bins(root, package_name, session)?
    } else {
        infer::directory(root, Path::new(directory), session)?
    };
    let context = merge::Context {
        root,
        autodiscover,
        edition,
        human,
        kind: label,
        flag,
    };
    let selected = match declared {
        Some(declared) => merge::select_prepared(
            Some(declared),
            inferred.as_slice(),
            context,
            warnings,
            session,
        )?,
        None => merge::select(original, inferred.as_slice(), context, warnings, session)?,
    };
    let binary = matches!(kind, resolve::Kind::Bin);
    if !binary {
        for draft in selected.as_slice() {
            check(draft, kind, edition, warnings, errors, session)?;
        }
    }
    let mut legacy_warnings = storage::Buffer::new();
    for draft in selected.into_iter() {
        if binary {
            check(&draft, kind, edition, warnings, errors, session)?;
        }
        let Some(name) = draft.name() else {
            return session.fail(Error::State);
        };
        let legacy = match kind {
            resolve::Kind::Bin => resolve::Legacy::Bin { has_lib },
            resolve::Kind::Bench => resolve::Legacy::Bench,
            _ => resolve::Legacy::None,
        };
        let resolved = resolve::resolve(
            name,
            draft.path(),
            inferred.as_slice(),
            kind,
            root,
            edition,
            legacy,
            session,
        )?;
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(error) if binary => return Ok(Err(error)),
            Err(error) => {
                errors.push(&[&error], session)?;
                continue;
            }
        };
        let (path, warning) = resolved.into_parts();
        if let Some(warning) = warning {
            if binary {
                warnings.push(&[&warning], session)?;
            } else {
                legacy_warnings.push(warning, session)?;
            }
        }
        let path = path::normalize(&path, session)?;
        push(draft, path, &mut output, session)?;
    }
    for warning in legacy_warnings.into_iter() {
        warnings.push(&[&warning], session)?;
    }
    Ok(Ok(output))
}

fn check(
    draft: &merge::Draft<'_, '_>,
    kind: resolve::Kind,
    edition: u16,
    warnings: &mut Warnings,
    errors: &mut validate::Errors,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<()> {
    let common = match kind {
        resolve::Kind::Bin => {
            return validate::binary(
                draft.original,
                draft.name(),
                edition,
                warnings,
                errors,
                session,
            )
        }
        resolve::Kind::Example => validate::Kind::Example,
        resolve::Kind::Test => validate::Kind::Test,
        resolve::Kind::Bench => validate::Kind::Bench,
    };
    validate::common(
        draft.original,
        draft.name(),
        common,
        edition,
        warnings,
        session,
    )
}
fn push<'a, 'd>(
    draft: merge::Draft<'a, 'd>,
    path: PathBuf,
    output: &mut storage::Buffer<Normalized<'a, 'd>>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<()> {
    let Some(name) = draft.name else {
        return session.fail(Error::State);
    };
    output.push(
        Normalized {
            original: draft.original,
            name,
            path,
        },
        session,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
