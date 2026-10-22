// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned explicit/inferred target selection, before validation/path resolution.

use super::super::super::{
    schema::{manifest::Targets, Target},
    session::Session,
    storage, Step,
};
use super::super::{path, Warnings};
use super::{files::Text, infer, target};
use rawr_toml::document;
use std::path::{Path, PathBuf};

/// Incomplete target fields; only the later validated/resolved stage completes it.
pub(super) struct Draft<'a, 'd> {
    pub(super) original: Option<&'a Target<'d>>,
    pub(super) name: Option<Text<'d>>,
    pub(super) path: Option<PathBuf>,
}
impl Draft<'_, '_> {
    pub(super) fn name(&self) -> Option<&str> {
        self.name.as_ref().map(Text::as_str)
    }
    pub(super) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

pub(super) struct Context<'a> {
    pub(super) root: &'a Path,
    pub(super) autodiscover: Option<bool>,
    pub(super) edition: u16,
    pub(super) human: &'a str,
    pub(super) kind: &'a str,
    pub(super) flag: &'a str,
}

pub(super) fn are_normalized<A: document::Admission>(
    original: Option<&Targets<'_>>,
    autodiscover: Option<bool>,
    session: &mut Session<'_, A>,
) -> Step<bool> {
    session.check()?;
    if autodiscover != Some(false) {
        return Ok(false);
    }
    if let Some(original) = original {
        for entry in original.entries() {
            if target::field(Some(entry.target()), "name", session)?.is_none()
                || target::field(Some(entry.target()), "path", session)?.is_none()
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub(super) fn declared<'a, 'd, A: document::Admission>(
    original: Option<&'a Targets<'d>>,
    session: &mut Session<'_, A>,
) -> Step<storage::Buffer<Draft<'a, 'd>>> {
    let mut output = storage::Buffer::new();
    if let Some(original) = original {
        for entry in original.entries() {
            let original = Some(entry.target());
            let name = target::field(original, "name", session)?;
            let name = target::string(name, session)?.map(Text::Borrowed);
            let declared = target::field(original, "path", session)?;
            let declared = target::string(declared, session)?;
            let path = declared
                .map(|text| path::join(Path::new(""), Path::new(text.as_str()), session))
                .transpose()?;
            output.push(
                Draft {
                    original,
                    name,
                    path,
                },
                session,
            )?;
        }
    }
    Ok(output)
}

pub(super) fn select<'a, 'd, A: document::Admission>(
    original: Option<&'a Targets<'d>>,
    inferred: &[infer::Item],
    context: Context<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<storage::Buffer<Draft<'a, 'd>>> {
    select_with(
        original.is_some(),
        inferred,
        context,
        warnings,
        session,
        |session| declared(original, session),
    )
}

/// Embedded manifests supply an admitted synthetic declaration before inference.
pub(super) fn select_prepared<'a, 'd, A: document::Admission>(
    original: Option<storage::Buffer<Draft<'a, 'd>>>,
    inferred: &[infer::Item],
    context: Context<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<storage::Buffer<Draft<'a, 'd>>> {
    select_with(
        original.is_some(),
        inferred,
        context,
        warnings,
        session,
        |_| Ok(original.unwrap_or_else(storage::Buffer::new)),
    )
}

fn select_with<'a, 'd, A: document::Admission>(
    explicit: bool,
    inferred: &[infer::Item],
    context: Context<'_>,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
    declared: impl FnOnce(&mut Session<'_, A>) -> Step<storage::Buffer<Draft<'a, 'd>>>,
) -> Step<storage::Buffer<Draft<'a, 'd>>> {
    session.check()?;
    let mut candidates = storage::Buffer::new();
    for item in inferred {
        let name = target::owned(&[item.name()], false, session)?;
        let path = path::join(Path::new(""), item.path(), session)?;
        candidates.push(
            Draft {
                original: None,
                name: Some(Text::Owned(name)),
                path: Some(path),
            },
            session,
        )?;
    }
    let mut output = declared(session)?;
    if explicit {
        let mut remaining = storage::Buffer::new();
        {
            let mut names = storage::Buffer::new();
            let mut paths = storage::Buffer::new();
            for item in output.as_slice() {
                names.push(item.name(), session)?;
                paths.push(
                    item.path()
                        .map(|path| path::join(context.root, path, session))
                        .transpose()?,
                    session,
                )?;
            }
            storage::sort(names.as_mut_slice(), |a, b| a.cmp(b));
            storage::sort(paths.as_mut_slice(), |a, b| a.cmp(b));
            for item in candidates.into_iter() {
                if names.as_slice().binary_search(&item.name()).is_ok() {
                    continue;
                }
                let joined = item
                    .path()
                    .map(|path| path::join(context.root, path, session))
                    .transpose()?;
                if paths.as_slice().binary_search(&joined).is_err() {
                    remaining.push(item, session)?;
                }
            }
        }
        let enabled = match context.autodiscover {
            Some(enabled) => enabled,
            None if context.edition == 2015 => {
                if remaining.len() != 0 {
                    warning(&context, remaining.as_slice(), warnings, session)?;
                }
                false
            }
            None => true,
        };
        if enabled {
            for item in remaining.into_iter() {
                output.push(item, session)?;
            }
        }
    } else if context.autodiscover != Some(false) {
        output = candidates;
    }
    // Cargo uses unstable sorting here; the fast declared path remains unsorted.
    output
        .as_mut_slice()
        .sort_unstable_by(|a, b| a.name().cmp(&b.name()));
    Ok(output)
}

fn warning<A: document::Admission>(
    context: &Context<'_>,
    remaining: &[Draft<'_, '_>],
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    struct Paths<'a, 'b, 'd>(&'a [Draft<'b, 'd>]);
    impl std::fmt::Display for Paths<'_, '_, '_> {
        fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for item in self.0 {
                if let Some(path) = item.path() {
                    writeln!(out, "* {}", path.display())?;
                }
            }
            Ok(())
        }
    }
    let text = target::formatted(format_args!(
        "An explicit [[{section}]] section is specified in Cargo.toml which currently\ndisables Cargo from automatically inferring other {human} targets.\nThis inference behavior will change in the Rust 2018 edition and the following\nfiles will be included as a {human} target:\n\n{paths}\nThis is likely to break cargo build or cargo test as these files may not be\nready to be compiled as a {human} target today. You can future-proof yourself\nand disable this warning by adding `{flag} = false` to your [package]\nsection. You may also move the files to a location where Cargo would not\nautomatically infer them to be a target, such as in subfolders.\n\nFor more information on this warning you can consult\nhttps://github.com/rust-lang/cargo/issues/5330",
        section=context.kind, human=context.human, flag=context.flag, paths=Paths(remaining)), session)?;
    warnings.push(&[&text], session)
}

#[cfg(test)]
mod tests;
