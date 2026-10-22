// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Pinned library normalization; see library/PROVENANCE.md.
//! This stage precedes binaries, other targets, dependencies and conversion.

use std::{
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use rawr_toml::document;

use super::super::super::{
    schema::{manifest, package as schema_package, primitive, Target},
    session::Session,
    Diagnostic, Error, Step, Tag,
};
use super::super::{path, Warnings};
use super::{
    files::Text,
    package,
    target::{field, owned, string},
};
use crate::input::membership::host;

/// Name/path are the only replacements. Every other typed field, including
/// both deprecated aliases, retains its original admitted representation.
pub(in super::super) struct Library<'a, 'd> {
    original: Option<&'a Target<'d>>,
    name: Text<'d>,
    path: PathBuf,
}
impl<'a, 'd> Library<'a, 'd> {
    pub(in super::super) fn name(&self) -> &str {
        self.name.as_str()
    }
    pub(in super::super) fn path(&self) -> &Path {
        &self.path
    }
    pub(in super::super) fn original(&self) -> Option<&'a Target<'d>> {
        self.original
    }
}

/// Successful library normalization only, never a loaded-manifest proof.
pub(in super::super) struct Complete<'a, 'd> {
    pub(super) package: package::Complete<'a, 'd>,
    pub(super) library: Option<Library<'a, 'd>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    pub(in super::super) fn package(&self) -> &package::Complete<'a, 'd> {
        &self.package
    }
    pub(in super::super) fn library(&self) -> Option<&Library<'a, 'd>> {
        self.library.as_ref()
    }
    /// normalize_features is an unchanged optional map before normalize_lib.
    /// Borrow the admitted map rather than cloning its entire projection.
    pub(in super::super) fn features(&self) -> Option<&'a manifest::Value<'d>> {
        self.package.package.as_ref()?;
        self.package
            .patches
            .preparing
            .original
            .get(manifest::Field::Features)
    }
}

impl<'a, 'd> package::Complete<'a, 'd> {
    pub(in super::super) fn normalize_library(
        mut self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Complete<'a, 'd>> {
        session.check()?;
        let Some(package) = self.package.as_ref() else {
            session.items(1)?;
            return Ok(Complete {
                package: self,
                library: None,
            });
        };
        let Some(package_name) = package.get(schema_package::Field::Name).and_then(text) else {
            return session.fail(Error::State);
        };
        let edition = package
            .get(schema_package::Field::Edition)
            .and_then(text)
            .unwrap_or("2015");
        // Cargo's parse().unwrap_or_default() falls back to 2015 here. Invalid
        // edition reporting belongs to the later real-manifest conversion.
        let edition = match edition {
            "2018" => 2018,
            "2021" => 2021,
            "2024" => 2024,
            _ => 2015,
        };
        let preparing = &mut self.patches.preparing;
        let original_package = match preparing
            .original
            .get(manifest::Field::Package)
            .or_else(|| preparing.original.get(manifest::Field::Project))
        {
            Some(manifest::Value::Package(package)) => Some(package),
            None => None,
            _ => return session.fail(Error::State),
        };
        let autodiscover = match original_package
            .and_then(|package| package.get(schema_package::Field::Autolib))
        {
            Some(schema_package::Value::Local(primitive::Value::Boolean(value))) => Some(*value),
            None => preparing.embedded.then_some(false),
            _ => return session.fail(Error::State),
        };
        let original = match preparing.original.get(manifest::Field::Lib) {
            Some(manifest::Value::Target(target)) => Some(target),
            None => None,
            _ => return session.fail(Error::State),
        };
        let Some(root) = preparing.path.parent() else {
            return session.fail(Error::State);
        };
        let library = normalize(
            original,
            root,
            package_name,
            edition,
            autodiscover,
            &mut preparing.configured.warnings,
            session,
        )?;
        session.items(1)?;
        Ok(Complete {
            package: self,
            library,
        })
    }
}

pub(in super::super) fn text<'v>(value: &'v package::Value<'_, '_>) -> Option<&'v str> {
    match value {
        package::Value::Text(text) => Some(text.as_str()),
        package::Value::Original(primitive::Value::String(text)) => Some(text.as_str()),
        package::Value::Original(primitive::Value::Name(name)) => Some(name.as_str()),
        _ => None,
    }
}

fn normalize<'a, 'd>(
    original: Option<&'a Target<'d>>,
    root: &Path,
    package_name: &str,
    edition: u16,
    autodiscover: Option<bool>,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<Library<'a, 'd>>> {
    session.check()?;
    // Determine the fast branch from presence, before any filesystem probe.
    let name = field(original, "name", session)?;
    let declared_path = field(original, "path", session)?;
    let normalized = autodiscover == Some(false)
        && (original.is_none() || name.is_some() && declared_path.is_some());
    if normalized && original.is_none() {
        return Ok(None);
    }
    let inferred = if normalized {
        false
    } else {
        let candidate = path::join(root, Path::new("src/lib.rs"), session)?;
        session.exists(candidate.as_os_str().as_bytes())?
    };
    if original.is_none() && !inferred {
        return Ok(None);
    }
    let name = match string(name, session)? {
        Some(name) => {
            session.items(1)?;
            Text::Borrowed(name)
        }
        None if normalized => return session.reject(Diagnostic::at(Tag::LibraryMissingName, None)),
        None => Text::Owned(owned(&[package_name], true, session)?),
    };
    session.string(name.as_str().len())?;
    if name.as_str().trim().is_empty() {
        return session.reject(Diagnostic::at(Tag::LibraryEmptyName, None));
    }
    if name.as_str().contains('-') {
        return session.reject(Diagnostic::at(Tag::LibraryHyphen, None));
    }
    deprecated(
        original,
        "proc_macro",
        "proc-macro",
        Tag::LibraryProcMacroUnderscore,
        name.as_str(),
        edition,
        warnings,
        session,
    )?;
    deprecated(
        original,
        "crate_type",
        "crate-type",
        Tag::LibraryCrateTypeUnderscore,
        name.as_str(),
        edition,
        warnings,
        session,
    )?;
    let declared_path = string(declared_path, session)?;
    let resolved = match declared_path {
        Some(text) => path::normalize(Path::new(text.as_str()), session)?,
        None if inferred => path::normalize(Path::new("src/lib.rs"), session)?,
        None => {
            // Construct the spelling before testing the edition, as Cargo does.
            let filename = owned(&[name.as_str(), ".rs"], false, session)?;
            let legacy = path::join(Path::new("src"), Path::new(&filename), session)?;
            if edition != 2015 {
                return session.reject(Diagnostic::at(Tag::LibraryMissingPath, None));
            }
            let candidate = path::join(root, &legacy, session)?;
            // Unix metadata rejects interior NUL before reaching the syscall;
            // Path::exists translates that source-local error to false. Such a
            // spelling must not be presented as a recorded native path.
            session.string(candidate.as_os_str().len())?;
            if candidate.as_os_str().as_bytes().contains(&0)
                || !session.exists(candidate.as_os_str().as_bytes())?
            {
                return session.reject(Diagnostic::at(Tag::LibraryMissingPath, None));
            }
            let Some(spelling) = legacy.to_str() else {
                return session.fail(Error::State);
            };
            warnings.push(
                &[
                    "path `",
                    spelling,
                    "` was erroneously implicitly accepted for library `",
                    name.as_str(),
                    "`,\nplease rename the file to `src/lib.rs` or set lib.path in Cargo.toml",
                ],
                session,
            )?;
            path::normalize(&legacy, session)?
        }
    };
    session.items(1)?;
    Ok(Some(Library {
        original,
        name,
        path: resolved,
    }))
}

fn deprecated<A: document::Admission>(
    original: Option<&Target<'_>>,
    old: &'static str,
    new: &'static str,
    tag: Tag,
    name: &str,
    edition: u16,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    if field(original, old, session)?.is_none() {
        return Ok(());
    }
    if edition >= 2024 {
        return session.reject(Diagnostic::at(tag, None));
    }
    if field(original, new, session)?.is_some() {
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
                "` library target",
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
                "` library target)",
            ],
            session,
        )
    }
}
