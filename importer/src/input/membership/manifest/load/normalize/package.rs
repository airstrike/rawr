// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Pinned util/toml/mod.rs:624–862 package transformation after patches.
//! Completion proves this stage only, not successful loading or membership.

use std::path::Path;

use super::super::super::{
    schema::{manifest, package, primitive, workspace as schema_workspace},
    session::Session,
    storage, value, Diagnostic, Error, Step, Tag,
};
use super::super::{features::Feature, path, roots, workspace};
use super::{files, name, Patches};
use crate::input::membership::host;

/// All untransformed payloads borrow their original admitted interpretation.
/// Text and Build own only actual replacements; inherited markers never escape.
pub(in super::super) enum Value<'a, 'd> {
    Original(&'a primitive::Value<'d>),
    Metadata(&'a value::Projection<'d>),
    Text(files::Text<'d>),
    Boolean(bool),
    Build(files::Build<'a, 'd>),
}

pub(in super::super) struct View<'a, 'd> {
    fields: storage::Buffer<(package::Field, Value<'a, 'd>)>,
}
impl<'a, 'd> View<'a, 'd> {
    /// Only the embedded default branch replaces Edition with Text; local and
    /// inherited declarations remain Original. Retains reached report intent.
    pub(in super::super) fn embedded_edition_defaulted(&self) -> bool {
        matches!(self.get(package::Field::Edition), Some(Value::Text(_)))
    }
    pub(in super::super) fn get(&self, field: package::Field) -> Option<&Value<'a, 'd>> {
        self.fields
            .as_slice()
            .iter()
            .find(|(key, _)| *key == field)
            .map(|(_, value)| value)
    }
    pub(in super::super) fn fields(
        &self,
    ) -> impl Iterator<Item = (package::Field, &Value<'a, 'd>)> {
        self.fields
            .as_slice()
            .iter()
            .map(|(field, value)| (*field, value))
    }
}

fn embedded_edition<A: rawr_toml::document::Admission>(
    session: &mut Session<'_, A>,
) -> Step<files::Text<'static>> {
    session.check()?;
    // Only util/toml/mod.rs:643–654's direct terminal report is classified
    // here. Its force=true rendering is not a stored manifest warning;
    // quiet mode only removes the help group. Omit that exact terminal closure,
    // not the branch, logical reached boundary, or semantic default before
    // RustVersion. The Edition Text replacement retains this branch's intent.
    session.items(1)?;
    // Cargo core/features.rs:206 pins LATEST_STABLE to Edition2024. These are
    // retained output bytes, not fictional renderer ownership or output success.
    session.string(4)?;
    Ok(files::Text::Borrowed(value::Text::Borrowed("2024")))
}

/// Retains the original flags for subsequent target normalization, alongside
/// the transformed package whose five auto flags are unconditionally false.
pub(in super::super) struct Complete<'a, 'd> {
    pub(super) patches: Patches<'a, 'd>,
    pub(super) package: Option<View<'a, 'd>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    pub(in super::super) fn patches(&self) -> &Patches<'a, 'd> {
        &self.patches
    }
    pub(in super::super) fn package(&self) -> Option<&View<'a, 'd>> {
        self.package.as_ref()
    }
}

pub(in super::super) enum Progress<'a, 'd> {
    Complete(Complete<'a, 'd>),
    Workspace(Workspace<'a, 'd>),
}

/// Suspends only at a reached inherited field. Already normalized fields and
/// file observations stay retained and are never replayed on resumption.
pub(in super::super) struct Workspace<'a, 'd> {
    cursor: Cursor<'a, 'd>,
}
impl<'a, 'd> Workspace<'a, 'd> {
    pub(in super::super) fn failure_context<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        use package::Field;
        let label = match ORDER.get(self.cursor.next) {
            Some(Field::Edition) => "edition",
            Some(Field::RustVersion) => "rust-version",
            Some(Field::Version) => "version",
            Some(Field::Authors) => "authors",
            Some(Field::Exclude) => "exclude",
            Some(Field::Include) => "include",
            Some(Field::Publish) => "publish",
            Some(Field::Description) => "description",
            Some(Field::Homepage) => "homepage",
            Some(Field::Documentation) => "documentation",
            Some(Field::Readme) => "readme",
            Some(Field::Keywords) => "keywords",
            Some(Field::Categories) => "categories",
            Some(Field::License) => "license",
            Some(Field::LicenseFile) => "license-file",
            Some(Field::Repository) => "repository",
            _ => return session.fail(Error::State),
        };
        super::target::formatted(format_args!("error inheriting `{label}` from workspace root manifest's `workspace.package.{label}`"), session)
    }
    pub(in super::super) fn manifest(&self) -> &'a Path {
        self.cursor.patches.preparing.path
    }
    pub(in super::super) fn explicit_root(&self) -> Option<&str> {
        match &self.cursor.patches.preparing.configured.workspace {
            workspace::Configuration::Member { root } => root.as_ref().map(|text| text.as_str()),
            _ => None,
        }
    }
    /// Supply the declaration selected for this manifest by Cargo's cache
    /// branch or successful full recursive loading, never a path-only proof or
    /// a bare Reader::prepare result. Preserve the patch/package shared memo.
    pub(in super::super) fn resume_declared(
        mut self,
        root: roots::Root<'a, 'd>,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Progress<'a, 'd>> {
        session.check()?;
        session.items(1)?;
        self.cursor.patches.workspace = Some(root);
        self.cursor.run(session)
    }
}

struct Cursor<'a, 'd> {
    patches: Patches<'a, 'd>,
    original: Option<&'a package::Package<'d>>,
    output: View<'a, 'd>,
    next: usize,
}

impl<'a, 'd> Patches<'a, 'd> {
    pub(in super::super) fn normalize_package(
        self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Progress<'a, 'd>> {
        session.check()?;
        let original = match self
            .preparing
            .original
            .get(manifest::Field::Package)
            .or_else(|| self.preparing.original.get(manifest::Field::Project))
        {
            Some(manifest::Value::Package(package)) => Some(package),
            None => None,
            _ => return session.fail(Error::State),
        };
        session.items(1)?;
        if original.is_none() && !self.preparing.embedded {
            return Ok(Progress::Complete(Complete {
                patches: self,
                package: None,
            }));
        }
        Cursor {
            patches: self,
            original,
            output: View {
                fields: storage::Buffer::new(),
            },
            next: 0,
        }
        .run(session)
    }
}

// This is source evaluation order, independent of declaration encounter order.
const ORDER: [package::Field; 32] = {
    use package::Field::*;
    [
        Edition,
        RustVersion,
        Name,
        Version,
        Authors,
        Build,
        Metabuild,
        DefaultTarget,
        ForcedTarget,
        Links,
        Exclude,
        Include,
        Publish,
        Workspace,
        ImATeapot,
        Autolib,
        Autobins,
        Autoexamples,
        Autotests,
        Autobenches,
        DefaultRun,
        Description,
        Homepage,
        Documentation,
        Readme,
        Keywords,
        Categories,
        License,
        LicenseFile,
        Repository,
        Resolver,
        Metadata,
    ]
};

impl<'a, 'd> Cursor<'a, 'd> {
    fn run(mut self, session: &mut Session<'_, host::Host<'_, '_>>) -> Step<Progress<'a, 'd>> {
        session.check()?;
        while let Some(&field) = ORDER.get(self.next) {
            let input = self.original.and_then(|original| original.get(field));
            if matches!(input, Some(package::Value::Inherited)) && self.patches.workspace.is_none()
            {
                match self.patches.preparing.configured.workspace {
                    workspace::Configuration::Root {
                        directory,
                        declaration,
                    } => {
                        session.items(1)?;
                        self.patches.workspace = Some(roots::Root {
                            directory,
                            declaration,
                        });
                    }
                    workspace::Configuration::Member { .. } => {
                        session.items(1)?;
                        return Ok(Progress::Workspace(Workspace { cursor: self }));
                    }
                }
            }
            session.items(1)?;
            if let Some(value) = self.field(field, input, session)? {
                self.output.fields.push((field, value), session)?;
            }
            self.next += 1;
        }
        session.items(1)?;
        Ok(Progress::Complete(Complete {
            patches: self.patches,
            package: Some(self.output),
        }))
    }

    fn field(
        &self,
        field: package::Field,
        input: Option<&'a package::Value<'d>>,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Option<Value<'a, 'd>>> {
        use package::Field;
        if field == Field::Build {
            if self.patches.preparing.embedded {
                return Ok(Some(Value::Build(files::Build::Auto(false))));
            }
            let build = match input {
                Some(package::Value::Local(primitive::Value::Build(build))) => Some(build),
                None => None,
                _ => return session.fail(Error::State),
            };
            if matches!(build, Some(primitive::Build::Multiple(_))) {
                self.patches
                    .preparing
                    .configured
                    .features
                    .require(Feature::MultipleBuildScripts, session)?;
            }
            return files::build(build, self.root(session)?, session)
                .map(|value| Some(Value::Build(value)));
        }
        if matches!(
            field,
            Field::Autolib
                | Field::Autobins
                | Field::Autoexamples
                | Field::Autotests
                | Field::Autobenches
        ) {
            return Ok(Some(Value::Boolean(false)));
        }
        if field == Field::Readme {
            return self.readme(input, session).map(Some);
        }
        match input {
            Some(package::Value::Local(value)) => Ok(Some(Value::Original(value))),
            Some(package::Value::Metadata(value)) => Ok(Some(Value::Metadata(value))),
            Some(package::Value::Inherited) => {
                let (root, value) = self.inherited(field, session)?;
                if field == Field::LicenseFile {
                    let primitive::Value::String(text) = value else {
                        return session.fail(Error::State);
                    };
                    return self
                        .rebase(field, root.directory, text.as_str(), session)
                        .map(|text| Some(Value::Text(files::Text::Owned(text))));
                }
                Ok(Some(Value::Original(value)))
            }
            None if field == Field::Name => {
                if self.patches.preparing.embedded {
                    name::embedded(self.patches.preparing.path, session)
                        .map(|text| Some(Value::Text(files::Text::Owned(text))))
                } else {
                    self.reject(Tag::PackageMissingName, field, session)
                }
            }
            None if field == Field::Edition && self.patches.preparing.embedded => {
                embedded_edition(session).map(|text| Some(Value::Text(text)))
            }
            None => Ok(None),
        }
    }

    fn root<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<&'a Path> {
        match self.patches.preparing.path.parent() {
            Some(path) => Ok(path),
            None => session.fail(Error::State),
        }
    }

    fn defaults<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<(roots::Root<'a, 'd>, Option<&'a package::Defaults<'d>>)> {
        let Some(root) = self.patches.workspace else {
            return session.fail(Error::State);
        };
        let defaults = match root
            .declaration
            .and_then(|declaration| declaration.get(schema_workspace::Field::Package))
        {
            Some(schema_workspace::Value::Package(defaults)) => Some(defaults),
            None => None,
            _ => return session.fail(Error::State),
        };
        Ok((root, defaults))
    }

    fn inherited<A: rawr_toml::document::Admission>(
        &self,
        field: package::Field,
        session: &mut Session<'_, A>,
    ) -> Step<(roots::Root<'a, 'd>, &'a primitive::Value<'d>)> {
        let (root, defaults) = self.defaults(session)?;
        match defaults.and_then(|defaults| defaults.get(field)) {
            Some(package::Value::Local(value)) => Ok((root, value)),
            None => self.reject(Tag::PackageInherited(field), field, session),
            _ => session.fail(Error::State),
        }
    }

    fn readme(
        &self,
        input: Option<&'a package::Value<'d>>,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Value<'a, 'd>> {
        let field = package::Field::Readme;
        if matches!(input, Some(package::Value::Inherited)) {
            let (root, defaults) = self.defaults(session)?;
            // Unlike every generic getter, Cargo probes workspace defaults even
            // when the workspace.package table itself is absent.
            let readme = match defaults.and_then(|defaults| defaults.get(field)) {
                Some(package::Value::Local(primitive::Value::StringOrBool(value))) => Some(value),
                None => None,
                _ => return session.fail(Error::State),
            };
            let Some(text) = files::readme(root.directory, readme, session)? else {
                return self.reject(Tag::PackageInherited(field), field, session);
            };
            let text = self.rebase(field, root.directory, text.as_str(), session)?;
            // The outer package readme normalization receives a string here:
            // it neither probes the member root nor renormalizes this path.
            return Ok(Value::Text(files::Text::Owned(text)));
        }
        let readme = match input {
            Some(package::Value::Local(primitive::Value::StringOrBool(value))) => Some(value),
            None => None,
            _ => return session.fail(Error::State),
        };
        Ok(match files::readme(self.root(session)?, readme, session)? {
            Some(text) => Value::Text(text),
            None => Value::Boolean(false),
        })
    }

    fn rebase<A: rawr_toml::document::Admission>(
        &self,
        field: package::Field,
        old: &Path,
        text: &str,
        session: &mut Session<'_, A>,
    ) -> Step<String> {
        match path::rebase(old, self.root(session)?, text, session)? {
            Ok(text) => Ok(text),
            Err(path::Invalid::Relative) => {
                self.reject(Tag::PackagePathRelative(field), field, session)
            }
            Err(path::Invalid::Utf8) => self.reject(Tag::PackagePathUtf8(field), field, session),
        }
    }

    fn reject<A: rawr_toml::document::Admission, T>(
        &self,
        tag: Tag,
        field: package::Field,
        session: &mut Session<'_, A>,
    ) -> Step<T> {
        let span = self.original.and_then(|original| {
            original
                .entries()
                .iter()
                .find(|entry| entry.field() == field)
                .and_then(|entry| entry.span())
                .or(original.span())
        });
        session.reject(Diagnostic::at(tag, span))
    }
}
