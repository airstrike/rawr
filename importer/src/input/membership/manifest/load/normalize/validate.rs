// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned target validation, targets.rs:1139–1260; no target conversion.

use rawr_toml::document;

use super::super::super::{
    schema::Target, session::Session, storage, value, Diagnostic, Error, Step, Tag,
};
use super::super::Warnings;
use super::target;

#[cfg(test)]
mod tests;

/// Cargo's accumulated critical reports remain separate from ordinary warnings.
/// Appending one does not reject an otherwise successful normalization stage.
pub(in super::super) struct Errors(Warnings);
impl Errors {
    pub(in super::super) const fn new() -> Self {
        Self(Warnings::new())
    }
    pub(in super::super) fn as_slice(&self) -> &[String] {
        self.0.as_slice()
    }
    pub(in super::super) fn into_warnings(self) -> Warnings {
        self.0
    }
    pub(in super::super) fn push<A: document::Admission>(
        &mut self,
        parts: &[&str],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0.push(parts, session)
    }
}

#[derive(Clone, Copy)]
pub(in super::super) enum Kind {
    Example,
    Test,
    Bench,
}
impl Kind {
    fn human(self) -> &'static str {
        match self {
            Self::Example => "example",
            Self::Test => "test",
            Self::Bench => "benchmark",
        }
    }
}

pub(in super::super) fn binary<A: document::Admission>(
    original: Option<&Target<'_>>,
    name: Option<&str>,
    edition: u16,
    warnings: &mut Warnings,
    errors: &mut Errors,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let name = checked_name(name, session)?;
    if ["deps", "examples", "build", "incremental"].contains(&name) {
        return session.reject(Diagnostic::at(Tag::BinaryConflictingName, None));
    }
    // A nonempty effective crate-type list reports critically and deliberately
    // skips deprecated alias validation, including in edition 2024.
    let crates = target::field(original, "crate-type", session)?.or(target::field(
        original,
        "crate_type",
        session,
    )?);
    if let Some(crates) = crates {
        let value::View::Sequence(mut cursor) = crates.view(session)? else {
            return session.fail(Error::State);
        };
        let mut values = storage::Buffer::new();
        while let Some((_, item)) = cursor.next(session)? {
            let value::View::Scalar(value::Kind::String(text)) = item.view(session)? else {
                return session.fail(Error::State);
            };
            values.push(text, session)?;
        }
        if values.as_slice().is_empty() {
            deprecated(
                original,
                "crate_type",
                "crate-type",
                Tag::TargetCrateTypeUnderscore,
                name,
                "binary",
                edition,
                warnings,
                session,
            )?;
        } else {
            let mut parts = storage::Buffer::new();
            parts.push("the target `", session)?;
            parts.push(name, session)?;
            parts.push(
                "` is a binary and can't have any crate-types set (currently \"",
                session,
            )?;
            for (index, text) in values.as_slice().iter().enumerate() {
                if index != 0 {
                    parts.push(", ", session)?;
                }
                parts.push(text.as_str(), session)?;
            }
            parts.push("\")", session)?;
            errors.push(parts.as_slice(), session)?;
        }
    }
    // false in the preferred spelling overrides true in the old spelling.
    let proc_macro = target::field(original, "proc-macro", session)?.or(target::field(
        original,
        "proc_macro",
        session,
    )?);
    let enabled = match proc_macro.map(|input| input.view(session)).transpose()? {
        Some(value::View::Scalar(value::Kind::Boolean(value))) => value,
        None => false,
        _ => return session.fail(Error::State),
    };
    if enabled {
        errors.push(
            &[
                "the target `",
                name,
                "` is a binary and can't have `proc-macro` set `true`",
            ],
            session,
        )?;
    } else {
        deprecated(
            original,
            "proc_macro",
            "proc-macro",
            Tag::TargetProcMacroUnderscore,
            name,
            "binary",
            edition,
            warnings,
            session,
        )?;
    }
    Ok(())
}

pub(in super::super) fn common<A: document::Admission>(
    original: Option<&Target<'_>>,
    name: Option<&str>,
    kind: Kind,
    edition: u16,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let name = checked_name(name, session)?;
    deprecated(
        original,
        "proc_macro",
        "proc-macro",
        Tag::TargetProcMacroUnderscore,
        name,
        kind.human(),
        edition,
        warnings,
        session,
    )?;
    deprecated(
        original,
        "crate_type",
        "crate-type",
        Tag::TargetCrateTypeUnderscore,
        name,
        kind.human(),
        edition,
        warnings,
        session,
    )
}

fn checked_name<'a, A: document::Admission>(
    name: Option<&'a str>,
    session: &mut Session<'_, A>,
) -> Step<&'a str> {
    session.check()?;
    let Some(name) = name else {
        return session.reject(Diagnostic::at(Tag::TargetMissingName, None));
    };
    session.string(name.len())?;
    if name.trim().is_empty() {
        return session.reject(Diagnostic::at(Tag::TargetEmptyName, None));
    }
    // The pinned supported Unix cfg does not run the Windows-reserved warning.
    Ok(name)
}

fn deprecated<A: document::Admission>(
    original: Option<&Target<'_>>,
    old: &'static str,
    new: &'static str,
    tag: Tag,
    name: &str,
    kind: &str,
    edition: u16,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    if target::field(original, old, session)?.is_none() {
        return Ok(());
    }
    if edition >= 2024 {
        return session.reject(Diagnostic::at(tag, None));
    }
    if target::field(original, new, session)?.is_some() {
        warnings.push(
            &[
                "`",
                old,
                "` is redundant with `",
                new,
                "`, preferring `",
                new,
                "` in the `",
                name,
                "` ",
                kind,
                " target",
            ],
            session,
        )
    } else {
        warnings.push(
            &[
                "`",
                old,
                "` is deprecated in favor of `",
                new,
                "` and will not work in the 2024 edition\n(in the `",
                name,
                "` ",
                kind,
                " target)",
            ],
            session,
        )
    }
}
