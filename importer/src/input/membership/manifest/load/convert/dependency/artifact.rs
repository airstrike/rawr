// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Pinned artifact dependency kinds and target construction.

use crate::input::membership::manifest::{
    load::normalize::{dependencies, inherit, target as format},
    schema::{dependency::Field, primitive},
    session::Session,
    storage, Error, Step,
};
use rawr_toml::document;

pub(in super::super::super) mod target;

#[derive(Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(super) enum Kind {
    AllBinaries,
    SelectedBinary(String),
    Cdylib,
    Staticlib,
}

pub(super) struct Artifact {
    pub(super) kinds: storage::Buffer<Kind>,
    pub(super) is_lib: bool,
}

pub(super) struct Context {
    pub(super) kind: dependencies::Kind,
    pub(super) bindeps: bool,
    pub(super) json_target_spec: bool,
}

fn boolean<A: document::Admission>(
    value: Option<inherit::Value<'_, '_>>,
    session: &mut Session<'_, A>,
) -> Step<Option<bool>> {
    match value {
        None => Ok(None),
        Some(inherit::Value::Original(primitive::Value::Boolean(value))) => Ok(Some(*value)),
        Some(inherit::Value::Boolean(value)) => Ok(Some(value)),
        _ => session.fail(Error::State),
    }
}

pub(super) fn convert<'a, 'd: 'a, A: document::Admission>(
    name: &'a str,
    get: impl Fn(Field) -> Option<inherit::Value<'a, 'd>>,
    context: Context,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a>, String>> {
    session.check()?;
    let artifact = match get(Field::Artifact) {
        None => None,
        Some(inherit::Value::Original(primitive::Value::Strings(values))) => {
            Some(values.as_slice())
        }
        _ => return session.fail(Error::State),
    };
    let is_lib = boolean(get(Field::Lib), session)?;
    let target = super::text(get(Field::Target), session)?;
    let Some(artifact) = artifact else {
        for (present, specifier) in [(is_lib.is_some(), "lib"), (target.is_some(), "target")] {
            if present {
                return format::formatted(format_args!("'{specifier}' specifier cannot be used without an 'artifact = …' value ({name})"),session).map(Err);
            }
        }
        return Ok(Ok(Progress::Complete(None)));
    };
    if !context.bindeps {
        return format::formatted(
            format_args!("`artifact = …` requires `-Z bindeps` ({name})"),
            session,
        )
        .map(Err);
    }
    let kinds = match kinds(artifact.iter().map(|value| value.as_str()), session)? {
        Ok(kinds) => kinds,
        Err(error) => return Ok(Err(error)),
    };
    let target = match target {
        Some(value) => match target::parse(value, context.json_target_spec, session)? {
            Ok(target::Progress::Ready(target)) => Some(target),
            Ok(target::Progress::Canonical(request)) => {
                return Ok(Ok(Progress::Canonical(Pending { name, request })))
            }
            Err(error) => return Ok(Err(error)),
        },
        None => None,
    };
    if !matches!(context.kind, dependencies::Kind::Build)
        && matches!(target, Some(target::Target::Assume))
    {
        return format::formatted(
            format_args!(
                "`target = \"target\"` in normal- or dev-dependencies has no effect ({name})"
            ),
            session,
        )
        .map(Err);
    }
    Ok(Ok(Progress::Complete(Some(Artifact {
        kinds,
        is_lib: is_lib.unwrap_or(false),
    }))))
}

pub(super) enum Progress<'a> {
    Complete(Option<Artifact>),
    Canonical(Pending<'a>),
}

/// Native acquisition and actual-producer-CWD association remain outstanding.
/// This value grants no successful artifact or dependency conversion.
pub(super) struct Pending<'a> {
    name: &'a str,
    request: target::Request<'a>,
}
impl Pending<'_> {
    pub(super) fn spelling(&self) -> &str {
        self.request.spelling()
    }
    pub(super) fn dependency_name(&self) -> &str {
        self.name
    }
}

fn kinds<'a, A: document::Admission>(
    inputs: impl Iterator<Item = &'a str>,
    session: &mut Session<'_, A>,
) -> Step<Result<storage::Buffer<Kind>, String>> {
    let mut kinds = storage::Buffer::new();
    for text in inputs {
        session.items(1)?;
        session.string(text.len())?;
        let kind = match text {
            "bin" => Kind::AllBinaries,
            "cdylib" => Kind::Cdylib,
            "staticlib" => Kind::Staticlib,
            _ => match text.strip_prefix("bin:") {
                Some(name) => {
                    Kind::SelectedBinary(format::formatted(format_args!("{name}"), session)?)
                }
                None => {
                    return format::formatted(
                        format_args!("'{text}' is not a valid artifact specifier"),
                        session,
                    )
                    .map(Err)
                }
            },
        };
        kinds.push(kind, session)?;
    }
    let mut all = false;
    for kind in kinds.as_slice() {
        session.items(1)?;
        if matches!(kind, Kind::AllBinaries) {
            all = true;
            break;
        }
    }
    if all {
        for kind in kinds.as_slice() {
            session.items(1)?;
            if matches!(kind, Kind::SelectedBinary(_)) {
                return format::formatted(format_args!("Cannot specify both 'bin' and 'bin:<name>' binary artifacts, as 'bin' selects all available binaries."),session).map(Err);
            }
        }
    }
    let mut sorted = storage::Buffer::new();
    for kind in kinds.as_slice() {
        sorted.push(kind, session)?;
    }
    storage::try_sort(sorted.as_mut_slice(), |left, right| {
        compare(left, right, session)
    })?;
    let mut duplicates = 0;
    for pair in sorted.as_slice().windows(2) {
        if compare(pair[0], pair[1], session)?.is_eq() {
            duplicates += 1;
        }
    }
    if duplicates != 0 {
        let suffix = if duplicates > 1 { "s" } else { "" };
        return format::formatted(
            format_args!("Found {duplicates} duplicate binary artifact{suffix}"),
            session,
        )
        .map(Err);
    }
    Ok(Ok(kinds))
}

fn compare<A: document::Admission>(
    left: &Kind,
    right: &Kind,
    session: &mut Session<'_, A>,
) -> Step<std::cmp::Ordering> {
    session.items(1)?;
    if let (Kind::SelectedBinary(left), Kind::SelectedBinary(right)) = (left, right) {
        session.string(left.len())?;
        session.string(right.len())?;
        // A lexicographic comparison can inspect the complete shorter byte
        // sequence. Charge this bound on every comparison, not only on input.
        session.items(left.len().min(right.len()))?;
    }
    Ok(left.cmp(right))
}

#[cfg(test)]
mod tests;
