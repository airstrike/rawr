// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned target path selection and diagnostics, under the original Session.

use super::super::{
    super::{session::Session, storage, Step},
    path,
};
use super::{infer, suggest, target};
use crate::input::membership::host;
use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub(in super::super) enum Kind {
    Bin,
    Example,
    Test,
    Bench,
}
impl Kind {
    fn text(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Example => "example",
            Self::Test => "test",
            Self::Bench => "bench",
        }
    }
    fn directory(self, wrong: bool) -> &'static str {
        match (self, wrong) {
            (Self::Bin, false) => "src/bin",
            (Self::Bin, true) => "src/bins",
            (Self::Example, false) => "examples",
            (Self::Example, true) => "example",
            (Self::Test, false) => "tests",
            (Self::Test, true) => "test",
            (Self::Bench, false) => "benches",
            (Self::Bench, true) => "bench",
        }
    }
}
#[derive(Clone, Copy)]
pub(in super::super) enum Legacy {
    None,
    Bin { has_lib: bool },
    Bench,
}
pub(in super::super) struct Resolved {
    path: PathBuf,
    warning: Option<String>,
}
impl Resolved {
    pub(in super::super) fn path(&self) -> &Path {
        &self.path
    }
    pub(in super::super) fn warning(&self) -> Option<&str> {
        self.warning.as_deref()
    }
    pub(in super::super) fn into_parts(self) -> (PathBuf, Option<String>) {
        (self.path, self.warning)
    }
}

pub(in super::super) fn resolve(
    name: &str,
    explicit: Option<&Path>,
    inferred: &[infer::Item],
    kind: Kind,
    root: &Path,
    edition: u16,
    legacy: Legacy,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Resolved, String>> {
    if let Some(explicit) = explicit {
        return selected(copy(explicit, session)?, None, session);
    }
    let name = target::owned(&[name], false, session)?;
    let mut found = [None, None];
    let mut count = 0;
    for item in inferred {
        session.items(1)?;
        session.string(name.len())?;
        session.string(item.name().len())?;
        if item.name() == name {
            found[count] = Some(copy(item.path(), session)?);
            count += 1;
            if count == 2 {
                break;
            }
        }
    }
    let [first, second] = found;
    let (first, second) = match (first, second) {
        (Some(path), None) => return selected(path, None, session),
        pair => pair,
    };
    if edition == 2015
        && let Some((path, warning)) = legacy_path(&name, root, legacy, session)?
    {
        return selected(path, Some(warning), session);
    }
    let message =
        match (first, second) {
            (Some(first), Some(second)) => {
                let first = relative_display(&first, root, session)?;
                let second = relative_display(&second, root, session)?;
                target::owned(&[
                "cannot infer path for `", &name, "` ", kind.text(),
                "\nCargo doesn't know which to use because multiple target files found at `",
                &first, "` and `", &second, "`.",
            ], false, session)?
            }
            (None, None) => missing(&name, kind, root, inferred, session)?,
            _ => return session.fail(super::super::super::Error::State),
        };
    Ok(Err(message))
}

fn selected(
    path: PathBuf,
    warning: Option<String>,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<Resolved, String>> {
    session.items(1)?;
    Ok(Ok(Resolved { path, warning }))
}
fn copy(value: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<PathBuf> {
    path::join(Path::new(""), value, session)
}
fn exists(value: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<bool> {
    let bytes = value.as_os_str().as_bytes();
    session.string(bytes.len())?;
    if bytes.contains(&0) {
        return Ok(false);
    }
    session.exists(bytes)
}
fn legacy_path(
    name: &str,
    root: &Path,
    legacy: Legacy,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<(PathBuf, String)>> {
    let mut selected_path = None;
    let (human, field) = match legacy {
        Legacy::None => return Ok(None),
        Legacy::Bin { has_lib } => {
            if !has_lib {
                let file = target::owned(&[name, ".rs"], false, session)?;
                let relative = path::join(Path::new("src"), Path::new(&file), session)?;
                let full = path::join(root, &relative, session)?;
                if exists(&full, session)? {
                    selected_path = Some(relative);
                }
            }
            for candidate in ["src/main.rs", "src/bin/main.rs"] {
                if selected_path.is_some() {
                    break;
                }
                let relative = copy(Path::new(candidate), session)?;
                let full = path::join(root, &relative, session)?;
                if exists(&full, session)? {
                    selected_path = Some(relative);
                }
            }
            ("binary", "bin")
        }
        Legacy::Bench => {
            let relative = copy(Path::new("src/bench.rs"), session)?;
            session.string(name.len())?;
            if name == "bench" {
                let full = path::join(root, &relative, session)?;
                if exists(&full, session)? {
                    selected_path = Some(relative);
                }
            }
            ("benchmark", "bench")
        }
    };
    let Some(path) = selected_path else {
        return Ok(None);
    };
    let spelling = display(&path, session)?;
    let warning = target::owned(
        &[
            "path `",
            &spelling,
            "` was erroneously implicitly accepted for ",
            human,
            " `",
            name,
            "`,\nplease set ",
            field,
            ".path in Cargo.toml",
        ],
        false,
        session,
    )?;
    Ok(Some((path, warning)))
}

fn possible(
    name: &str,
    kind: Kind,
    wrong: bool,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<[PathBuf; 2]> {
    let directory = Path::new(kind.directory(wrong));
    let filename = target::owned(&[name, ".rs"], false, session)?;
    let file = path::join(directory, Path::new(&filename), session)?;
    let subdir = path::join(directory, Path::new(name), session)?;
    let main = path::join(&subdir, Path::new("main.rs"), session)?;
    Ok([file, main])
}
fn missing(
    name: &str,
    kind: Kind,
    root: &Path,
    inferred: &[infer::Item],
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<String> {
    let wrong = possible(name, kind, true, session)?;
    let possible = possible(name, kind, false, session)?;
    let suggestion = match suggest::closest(name, inferred, session)? {
        Some(closest) => target::owned(
            &[
                "\n\nhelp: a ",
                kind.text(),
                " with a similar name exists: `",
                closest,
                "`",
            ],
            false,
            session,
        )?,
        None => String::new(),
    };
    for (wrong, possible) in wrong.iter().zip(&possible) {
        let full = path::join(root, wrong, session)?;
        if exists(&full, session)? {
            let wrong = display(wrong, session)?;
            let possible = display(possible, session)?;
            return target::owned(
                &[
                    "can't find `",
                    name,
                    "` ",
                    kind.text(),
                    " at default paths, but found a file at `",
                    &wrong,
                    "`.\nPerhaps rename the file to `",
                    &possible,
                    "` for target auto-discovery, or specify ",
                    kind.text(),
                    ".path if you want to use a non-default path.",
                    &suggestion,
                ],
                false,
                session,
            );
        }
    }
    let file = display(&possible[0], session)?;
    let directory = display(&possible[1], session)?;
    target::owned(
        &[
            "can't find `",
            name,
            "` ",
            kind.text(),
            " at `",
            &file,
            "` or `",
            &directory,
            "`. Please specify ",
            kind.text(),
            ".path if you want to use a non-default path.",
            &suggestion,
        ],
        false,
        session,
    )
}
fn relative_display(
    value: &Path,
    root: &Path,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<String> {
    session.string(value.as_os_str().len())?;
    session.string(root.as_os_str().len())?;
    display(value.strip_prefix(root).unwrap_or(value), session)
}
fn display(value: &Path, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<String> {
    let bytes = value.as_os_str().as_bytes();
    session.string(bytes.len())?;
    let mut parts = storage::Buffer::new();
    for chunk in bytes.utf8_chunks() {
        parts.push(chunk.valid(), session)?;
        if !chunk.invalid().is_empty() {
            parts.push("\u{fffd}", session)?;
        }
    }
    target::owned(parts.as_slice(), false, session)
}

#[cfg(test)]
mod tests;
