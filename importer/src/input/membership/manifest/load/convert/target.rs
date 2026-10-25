// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Ordinary target conversion; custom build targets are a following stage.

pub(in super::super) mod configure;

use super::super::super::{
    schema::Target as Original, session::Session, storage, value, Error, Step,
};
use super::super::{
    features::{Feature, Features},
    normalize::{all_targets, library, resolve, target as field},
    path, Warnings,
};
use super::edition;
use rawr_toml::document;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Known {
    Bin,
    Lib,
    Rlib,
    Dylib,
    Cdylib,
    Staticlib,
    ProcMacro,
}
pub(in super::super) enum Crate<'d> {
    Known(Known),
    Other(value::Text<'d>),
}
impl Crate<'_> {
    pub(in super::super) fn text(&self) -> &str {
        match self {
            Self::Known(known) => match known {
                Known::Bin => "bin",
                Known::Lib => "lib",
                Known::Rlib => "rlib",
                Known::Dylib => "dylib",
                Known::Cdylib => "cdylib",
                Known::Staticlib => "staticlib",
                Known::ProcMacro => "proc-macro",
            },
            Self::Other(text) => text.as_str(),
        }
    }
}
pub(in super::super) struct Target<'a, 'd> {
    name: &'a str,
    path: PathBuf,
    kind: configure::Kind,
    crates: storage::Buffer<Crate<'d>>,
    example_library: bool,
    filename: Option<value::Text<'d>>,
    required: Option<storage::Buffer<value::Text<'d>>>,
    inferred: bool,
    settings: configure::Settings,
}
impl<'a, 'd> Target<'a, 'd> {
    pub(in super::super) fn name(&self) -> &str {
        self.name
    }
    pub(in super::super) fn path(&self) -> &Path {
        &self.path
    }
    pub(in super::super) fn kind(&self) -> configure::Kind {
        self.kind
    }
    pub(in super::super) fn crates(&self) -> &[Crate<'d>] {
        self.crates.as_slice()
    }
    pub(in super::super) fn example_library(&self) -> bool {
        self.example_library
    }
    pub(in super::super) fn filename(&self) -> Option<&str> {
        self.filename.as_ref().map(value::Text::as_str)
    }
    pub(in super::super) fn required(&self) -> Option<&[value::Text<'d>]> {
        self.required.as_ref().map(storage::Buffer::as_slice)
    }
    pub(in super::super) fn name_inferred(&self) -> bool {
        self.inferred
    }
    pub(in super::super) fn settings(&self) -> &configure::Settings {
        &self.settings
    }
}
pub(in super::super) enum Failure {
    Semantic(String),
    Edition,
}

pub(super) fn boolean<A: document::Admission>(
    original: Option<&Original<'_>>,
    name: &str,
    session: &mut Session<'_, A>,
) -> Step<Option<bool>> {
    let Some(input) = field::field(original, name, session)? else {
        return Ok(None);
    };
    match input.view(session)? {
        value::View::Scalar(value::Kind::Boolean(value)) => Ok(Some(value)),
        _ => session.fail(Error::State),
    }
}
pub(super) fn strings<'d, A: document::Admission>(
    original: Option<&Original<'d>>,
    name: &str,
    session: &mut Session<'_, A>,
) -> Step<Option<storage::Buffer<value::Text<'d>>>> {
    let Some(input) = field::field(original, name, session)? else {
        return Ok(None);
    };
    let value::View::Sequence(mut cursor) = input.view(session)? else {
        return session.fail(Error::State);
    };
    let mut output = storage::Buffer::new();
    while let Some((_, item)) = cursor.next(session)? {
        let value::View::Scalar(value::Kind::String(text)) = item.view(session)? else {
            return session.fail(Error::State);
        };
        output.push(text, session)?;
    }
    Ok(Some(output))
}
fn crate_types<'d, A: document::Admission>(
    original: Option<&Original<'d>>,
    session: &mut Session<'_, A>,
) -> Step<Option<storage::Buffer<value::Text<'d>>>> {
    match strings(original, "crate-type", session)? {
        Some(types) => Ok(Some(types)),
        None => strings(original, "crate_type", session),
    }
}
fn convert_crate(text: value::Text<'_>) -> Crate<'_> {
    let known = match text.as_str() {
        "bin" => Known::Bin,
        "lib" => Known::Lib,
        "rlib" => Known::Rlib,
        "dylib" => Known::Dylib,
        "cdylib" => Known::Cdylib,
        "staticlib" => Known::Staticlib,
        // This spelling is deliberately the pin's From<&String>, not as_str.
        "procmacro" => Known::ProcMacro,
        _ => return Crate::Other(text),
    };
    Crate::Known(known)
}

pub(in super::super) fn library<'a, 'd, A: document::Admission>(
    lib: Option<&'a library::Library<'a, 'd>>,
    root: &Path,
    edition: edition::Edition,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<Option<Target<'a, 'd>>, Failure>> {
    session.check()?;
    let Some(lib) = lib else {
        return Ok(Ok(None));
    };
    let path = path::join(root, lib.path(), session)?;
    let types = crate_types(lib.original(), session)?;
    let proc_macro = boolean(lib.original(), "proc-macro", session)?.or(boolean(
        lib.original(),
        "proc_macro",
        session,
    )?);
    let mut crates = storage::Buffer::new();
    if let Some(types) = types {
        let has = |name: &str| types.as_slice().iter().any(|value| value.as_str() == name);
        let Some(work) = types.len().checked_mul(3) else {
            return session.fail(Error::Overflow);
        };
        session.items(work)?;
        if has("dylib") && has("cdylib") {
            return field::formatted(
                format_args!(
                    "library `{}` cannot set the crate type of both `dylib` and `cdylib`",
                    lib.name()
                ),
                session,
            )
            .map(|text| Err(Failure::Semantic(text)));
        }
        if has("proc-macro") {
            warnings.push(
                &[
                    "library `",
                    lib.name(),
                    "` should only specify `proc-macro = true` instead of setting `crate-type`",
                ],
                session,
            )?;
            if types.len() > 1 {
                return field::formatted(
                    format_args!("cannot mix `proc-macro` crate type with others"),
                    session,
                )
                .map(|text| Err(Failure::Semantic(text)));
            }
            crates.push(Crate::Known(Known::ProcMacro), session)?;
        } else {
            for text in types.into_iter() {
                crates.push(convert_crate(text), session)?;
            }
        }
    } else {
        crates.push(
            Crate::Known(if proc_macro == Some(true) {
                Known::ProcMacro
            } else {
                Known::Lib
            }),
            session,
        )?;
    }
    let settings = match configure::configure(
        lib.original(),
        lib.name(),
        configure::Kind::Lib,
        edition,
        warnings,
        session,
    )? {
        Ok(settings) => settings,
        Err(_) => return Ok(Err(Failure::Edition)),
    };
    let inferred = field::field(lib.original(), "name", session)?.is_none();
    Ok(Ok(Some(Target {
        name: lib.name(),
        path,
        kind: configure::Kind::Lib,
        crates,
        example_library: false,
        filename: None,
        required: None,
        inferred,
        settings,
    })))
}

pub(in super::super) fn group<'a, 'd, A: document::Admission>(
    groups: &'a all_targets::Groups<'a, 'd>,
    kind: resolve::Kind,
    features: &Features<'_, '_>,
    root: &Path,
    edition: edition::Edition,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<storage::Buffer<Target<'a, 'd>>, Failure>> {
    session.check()?;
    let targets = groups.get(kind);
    let (kind, unique) = match kind {
        resolve::Kind::Bin => (configure::Kind::Bin, "binary"),
        resolve::Kind::Example => (configure::Kind::Example, "example"),
        resolve::Kind::Test => (configure::Kind::Test, "test"),
        resolve::Kind::Bench => (configure::Kind::Bench, "bench"),
    };
    if matches!(kind, configure::Kind::Bin) {
        for target in targets {
            if field::field(target.original(), "filename", session)?.is_some() {
                features.require(Feature::DifferentBinaryName, session)?;
            }
        }
    }
    let mut names = storage::Buffer::new();
    for target in targets {
        session.items(1)?;
        let name = target.name();
        match names.as_slice().binary_search(&name) {
            Ok(_) => return field::formatted(format_args!("found duplicate {unique} name {name}, but all {unique} targets must have a unique name"),session).map(|text|Err(Failure::Semantic(text))),
            Err(index) => {
                session.items(names.len() - index)?;
                names.push(name, session)?;
                names.as_mut_slice()[index..].rotate_right(1);
            }
        }
    }
    let mut output = storage::Buffer::new();
    for target in targets {
        let path = path::join(root, target.path(), session)?;
        let mut crates = storage::Buffer::new();
        if matches!(kind, configure::Kind::Example)
            && let Some(types) = crate_types(target.original(), session)?
        {
            for text in types.into_iter() {
                crates.push(convert_crate(text), session)?;
            }
        }
        let example_library = matches!(kind, configure::Kind::Example)
            && crates
                .as_slice()
                .iter()
                .any(|kind| !matches!(kind, Crate::Known(Known::Bin)));
        if matches!(kind, configure::Kind::Example) && !example_library {
            crates = storage::Buffer::new();
        }
        let filename = if matches!(kind, configure::Kind::Bin) {
            let value = field::field(target.original(), "filename", session)?;
            field::string(value, session)?
        } else {
            None
        };
        let required = strings(target.original(), "required-features", session)?;
        let settings = match configure::configure(
            target.original(),
            target.name(),
            kind,
            edition,
            warnings,
            session,
        )? {
            Ok(settings) => settings,
            Err(_) => return Ok(Err(Failure::Edition)),
        };
        output.push(
            Target {
                name: target.name(),
                path,
                kind,
                crates,
                example_library,
                filename,
                required,
                inferred: false,
                settings,
            },
            session,
        )?;
    }
    Ok(Ok(output))
}
