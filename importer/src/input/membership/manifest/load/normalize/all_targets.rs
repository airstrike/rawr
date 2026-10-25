// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Library followed by four target groups, Cargo util/toml/mod.rs:393–451.
//! Dependencies and real/virtual conversion remain required after this stage.

use super::super::super::{
    schema::{manifest, package, primitive},
    session::Session,
    storage, Error, Step,
};
use super::super::{config, Warnings};
use super::{library, resolve, tail, targets, validate, virtual_fields};
use crate::{cargo, input::membership::host};
use std::path::Path;

pub(in super::super) struct Groups<'a, 'd> {
    bins: storage::Buffer<targets::Normalized<'a, 'd>>,
    examples: storage::Buffer<targets::Normalized<'a, 'd>>,
    tests: storage::Buffer<targets::Normalized<'a, 'd>>,
    benches: storage::Buffer<targets::Normalized<'a, 'd>>,
}
impl<'a, 'd> Groups<'a, 'd> {
    pub(in super::super) fn get(&self, kind: resolve::Kind) -> &[targets::Normalized<'a, 'd>] {
        match kind {
            resolve::Kind::Bin => self.bins.as_slice(),
            resolve::Kind::Example => self.examples.as_slice(),
            resolve::Kind::Test => self.tests.as_slice(),
            resolve::Kind::Bench => self.benches.as_slice(),
        }
    }
}

/// Virtual manifests have no package target groups; this proves no conversion.
pub(in super::super) struct Complete<'a, 'd> {
    library: library::Complete<'a, 'd>,
    groups: Option<Groups<'a, 'd>>,
}
impl<'a, 'd> Complete<'a, 'd> {
    pub(in super::super) fn library(&self) -> &library::Complete<'a, 'd> {
        &self.library
    }
    pub(in super::super) fn groups(&self) -> Option<&Groups<'a, 'd>> {
        self.groups.as_ref()
    }

    /// Retain stable normalization inputs while the caller owns the report
    /// channel across lazy recursive workspace loading.
    pub(in super::super) fn into_tail(mut self) -> (Owner<'a, 'd>, Warnings) {
        let warnings = std::mem::replace(
            &mut self.library.package.patches.preparing.configured.warnings,
            Warnings::new(),
        );
        (Owner { targets: self }, warnings)
    }
}

/// Owns the inputs borrowed by a suspended tail; no self-referential cursor.
pub(in super::super) struct Owner<'a, 'd> {
    targets: Complete<'a, 'd>,
}
impl<'a, 'd> Owner<'a, 'd> {
    pub(in super::super) fn manifest(&self) -> &'a Path {
        self.targets.library.package.patches.preparing.path
    }
    pub(in super::super) fn explicit_root(&self) -> Option<&str> {
        match self.workspace() {
            super::super::workspace::Configuration::Member { root } => {
                root.as_ref().map(|root| root.as_str())
            }
            _ => None,
        }
    }
    pub(in super::super) fn workspace(&self) -> &super::super::workspace::Configuration<'a, 'd> {
        &self
            .targets
            .library
            .package
            .patches
            .preparing
            .configured
            .workspace
    }
    /// Called after virtual normalization has passed its package-field checks.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn convert_virtual<'b, 's: 'b, 'image: 's>(
        &'b self,
        specs: &mut storage::slots::Writer<'b, super::super::super::spec::Spec>,
        parent: super::super::convert::source::Id<'s>,
        sources: &mut super::super::convert::source::Owner<'s>,
        config: &mut config::Owner<'s, 'image>,
        bindeps: bool,
        json_target_spec: bool,
        limits: &cargo::Limits,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<
        Result<
            super::super::convert::workspace::Progress<'b, 's>,
            super::super::convert::replace::list::Failure,
        >,
    > {
        session.check()?;
        if self.targets.library.package.package().is_some() {
            return session.fail(Error::State);
        }
        let patches = &self.targets.library.package.patches;
        if patches.original().get(manifest::Field::Workspace).is_none() {
            return super::target::formatted(
                format_args!("manifest is missing either a `[package]` or a `[workspace]`"),
                session,
            )
            .map(|message| {
                Err(super::super::convert::replace::list::Failure::Conversion(
                    message,
                ))
            });
        }
        let preparing = &patches.preparing;
        super::super::convert::workspace::convert(
            patches.original(),
            patches.patch(),
            &preparing.configured.workspace,
            specs,
            preparing.path,
            parent,
            sources,
            config,
            bindeps,
            json_target_spec,
            preparing.configured.profiles,
            &preparing.configured.features,
            limits,
            warnings,
            session,
        )
    }
    /// Dependency conversion after target and original-dependency validation.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn convert_dependencies<'b, 's: 'b, 'image: 's>(
        &'b self,
        tail: &'b tail::Complete<'b, 'd>,
        targets: &super::super::convert::Targets<'_, '_>,
        specs: &mut storage::slots::Writer<'b, super::super::super::spec::Spec>,
        packages: &mut super::super::convert::package::Owner<'b, 's>,
        parent: super::super::convert::source::Id<'s>,
        sources: &mut super::super::convert::source::Owner<'s>,
        config: &mut config::Owner<'s, 'image>,
        bindeps: bool,
        json_target_spec: bool,
        limits: &cargo::Limits,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<
        Result<
            super::super::convert::dependencies::all::Progress<'b, 'd, 's>,
            super::super::convert::replace::list::Failure,
        >,
    > {
        let patches = &self.targets.library.package.patches;
        super::super::convert::dependencies::all::convert(
            tail,
            patches.original(),
            self.targets.library.package.package(),
            targets,
            patches.patch(),
            specs,
            packages,
            patches.preparing.path,
            parent,
            sources,
            config,
            bindeps,
            json_target_spec,
            patches.preparing.configured.lint_context,
            patches.preparing.configured.profiles,
            &patches.preparing.configured.features,
            limits,
            warnings,
            session,
        )
    }
    /// Original dependency diagnostics follow successful target/link checks.
    pub(in super::super) fn validate_dependencies(
        &self,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<(), String>> {
        let preparing = &self.targets.library.package.patches.preparing;
        super::super::convert::dependencies::original(
            preparing.original,
            preparing.path,
            warnings,
            session,
        )
    }
    pub(in super::super) fn targets(&self) -> &Complete<'a, 'd> {
        &self.targets
    }

    /// Post-target checks in mod.rs:1527–1559, before dependency validation.
    pub(in super::super) fn validate_targets(
        &self,
        targets: &super::super::convert::Targets<'_, '_>,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<(), String>> {
        session.check()?;
        if targets.ordinary.as_slice().is_empty() {
            return super::target::formatted(format_args!("no targets specified in the manifest\neither src/lib.rs, src/main.rs, a [lib] section, or [[bin]] section must be present"),session).map(Err);
        }
        let package = &self.targets.library.package;
        let Some(root) = package.patches.preparing.path.parent() else {
            return session.fail(Error::State);
        };
        super::super::convert::conflicts::inspect(targets, root, warnings, session)?;
        let Some(view) = package.package.as_ref() else {
            return session.fail(Error::State);
        };
        if let Some(links) = view.get(package::Field::Links).and_then(library::text)
            && targets.builds.as_slice().is_empty()
        {
            return super::target::formatted(format_args!("package specifies that it links to `{links}` but does not have a custom build script"),session).map(Err);
        }
        Ok(Ok(()))
    }

    pub(in super::super) fn convert_targets<'s>(
        &'s self,
        header: &super::super::convert::Header,
        tail: &super::tail::Complete<'_, 'd>,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<super::super::convert::Targets<'s, 'd>, super::super::convert::target::Failure>>
    {
        use super::super::convert::{self, target::Failure};
        let ordinary = match self.ordinary_targets(header, warnings, session)? {
            Ok(targets) => targets,
            Err(error) => return Ok(Err(error)),
        };
        let package = &self.targets.library.package;
        let Some(view) = package.package.as_ref() else {
            return session.fail(Error::State);
        };
        let Some(name) = view.get(package::Field::Name).and_then(library::text) else {
            return session.fail(Error::State);
        };
        let Some(root) = package.patches.preparing.path.parent() else {
            return session.fail(Error::State);
        };
        let Some(super::package::Value::Build(build)) = view.get(package::Field::Build) else {
            return session.fail(Error::State);
        };
        let metabuild = match view.get(package::Field::Metabuild) {
            Some(super::package::Value::Original(primitive::Value::Strings(values))) => {
                Some(values.as_slice())
            }
            None => None,
            _ => return session.fail(Error::State),
        };
        match convert::build::convert(
            build,
            metabuild,
            tail.maps().get(super::dependencies::Kind::Build),
            name,
            root,
            header.edition,
            session,
        )? {
            Ok(builds) => Ok(Ok(convert::Targets { ordinary, builds })),
            Err(error) => Ok(Err(Failure::Semantic(error))),
        }
    }

    /// Source-ordered ordinary targets; custom builds/metabuild and the rest of
    /// manifest conversion still follow before any Loaded authority.
    pub(in super::super) fn ordinary_targets<'s>(
        &'s self,
        header: &super::super::convert::Header,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<
        Result<
            storage::Buffer<super::super::convert::target::Target<'s, 'd>>,
            super::super::convert::target::Failure,
        >,
    > {
        use super::super::convert::target;
        session.check()?;
        let preparing = &self.targets.library.package.patches.preparing;
        let Some(root) = preparing.path.parent() else {
            return session.fail(Error::State);
        };
        let mut output = storage::Buffer::new();
        match target::library(
            self.targets.library.library(),
            root,
            header.edition,
            warnings,
            session,
        )? {
            Ok(Some(target)) => {
                output.push(target, session)?;
            }
            Ok(None) => {}
            Err(error) => return Ok(Err(error)),
        }
        let Some(groups) = self.targets.groups() else {
            return session.fail(Error::State);
        };
        for kind in [
            resolve::Kind::Bin,
            resolve::Kind::Example,
            resolve::Kind::Test,
            resolve::Kind::Bench,
        ] {
            match target::group(
                groups,
                kind,
                &preparing.configured.features,
                root,
                header.edition,
                warnings,
                session,
            )? {
                Ok(targets) => {
                    for target in targets.into_iter() {
                        output.push(target, session)?;
                    }
                }
                Err(error) => return Ok(Err(error)),
            }
        }
        Ok(Ok(output))
    }

    /// Continue real conversion through embedded restrictions and explicit
    /// resolver selection, before target conversion and manifest construction.
    pub(in super::super) fn conversion_header(
        &self,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<super::super::convert::Header, super::super::convert::preamble::Failure>> {
        use super::super::super::schema::workspace;
        use super::super::convert::{self, preamble::Failure};
        let edition = match self.preamble(warnings, session)? {
            Ok(edition) => edition,
            Err(error) => return Ok(Err(error)),
        };
        let package = &self.targets.library.package;
        let preparing = &package.patches.preparing;
        if preparing.embedded
            && let Err(error) = convert::embedded::check(preparing.original, session)?
        {
            return Ok(Err(Failure::Semantic(error)));
        }
        let Some(view) = package.package.as_ref() else {
            return session.fail(Error::State);
        };
        let resolver = view.get(package::Field::Resolver).and_then(library::text);
        // normalize_toml preserves the original workspace; its synthetic
        // embedded default has no resolver and cannot add an explicit value.
        let workspace = match preparing.original.get(manifest::Field::Workspace) {
            Some(manifest::Value::Workspace(workspace)) => {
                match workspace.get(workspace::Field::Resolver) {
                    Some(workspace::Value::Local(primitive::Value::String(text))) => {
                        Some(text.as_str())
                    }
                    None => None,
                    _ => return session.fail(Error::State),
                }
            }
            None => None,
            _ => return session.fail(Error::State),
        };
        match convert::resolver::select(resolver, workspace, session)? {
            Ok(resolver) => Ok(Ok(convert::Header { edition, resolver })),
            Err(error) => Ok(Err(Failure::Semantic(error))),
        }
    }

    /// Real conversion's initial checks only; the closed driver must complete
    /// the tail first and still perform all following conversion stages.
    pub(in super::super) fn preamble(
        &self,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<
        Result<super::super::convert::edition::Edition, super::super::convert::preamble::Failure>,
    > {
        let package = &self.targets.library.package;
        let preparing = &package.patches.preparing;
        let Some(view) = package.package.as_ref() else {
            return session.fail(Error::State);
        };
        let Some(root) = preparing.path.parent() else {
            return session.fail(Error::State);
        };
        super::super::convert::preamble::check(
            super::super::convert::preamble::Context {
                original: preparing.original,
                package: view,
                root,
                features: &preparing.configured.features,
            },
            warnings,
            session,
        )
    }

    /// None means the virtual normalization branch passed. Neither branch
    /// authorizes Loaded until its subsequent manifest conversion succeeds.
    pub(in super::super) fn normalize<'s, 'image: 'config, 'config>(
        &'s self,
        public_dependency: bool,
        warnings: &mut Warnings,
        owner: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Option<tail::Progress<'s, 'd>>, String>> {
        session.check()?;
        let package = &self.targets.library.package;
        let preparing = &package.patches.preparing;
        let Some(view) = package.package.as_ref() else {
            return virtual_fields::check(preparing.original, session)
                .map(|result| result.map(|()| None));
        };
        let Some(package_name) = view.get(package::Field::Name).and_then(library::text) else {
            return session.fail(Error::State);
        };
        let edition = match view.get(package::Field::Edition).and_then(library::text) {
            Some("2018") => 2018,
            Some("2021") => 2021,
            Some("2024") => 2024,
            _ => 2015,
        };
        let Some(package_root) = preparing.path.parent() else {
            return session.fail(Error::State);
        };
        let progress = tail::normalize(
            preparing.original,
            tail::Context {
                package_name,
                package_root,
                edition,
                features: &preparing.configured.features,
                public_dependency,
            },
            package.patches.workspace,
            warnings,
            owner,
            limits,
            session,
        )?;
        Ok(Ok(Some(progress)))
    }
}

impl<'a, 'd> library::Complete<'a, 'd> {
    pub(in super::super) fn normalize_targets(
        mut self,
        errors: &mut validate::Errors,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Complete<'a, 'd>, String>> {
        session.check()?;
        let Some(package) = self.package.package.as_ref() else {
            session.items(1)?;
            return Ok(Ok(Complete {
                library: self,
                groups: None,
            }));
        };
        let Some(name) = package.get(package::Field::Name).and_then(library::text) else {
            return session.fail(Error::State);
        };
        let edition = match package.get(package::Field::Edition).and_then(library::text) {
            Some("2018") => 2018,
            Some("2021") => 2021,
            Some("2024") => 2024,
            _ => 2015,
        };
        let preparing = &mut self.package.patches.preparing;
        let Some(root) = preparing.path.parent() else {
            return session.fail(Error::State);
        };
        let original = preparing.original;
        let original_package = match original
            .get(manifest::Field::Package)
            .or_else(|| original.get(manifest::Field::Project))
        {
            Some(manifest::Value::Package(package)) => Some(package),
            None if preparing.embedded => None,
            _ => return session.fail(Error::State),
        };
        let embedded = preparing.embedded;
        let bins = if embedded {
            let Some(filename) = preparing.path.file_name() else {
                return session.fail(Error::State);
            };
            targets::Input::Embedded {
                name,
                path: Path::new(filename),
            }
        } else {
            targets::Input::Declared(declared(original, manifest::Field::Bin, session)?)
        };
        // Every call completes before examining the following group's flag or
        // declarations, preserving rejection and filesystem observation order.
        macro_rules! group {
            ($input:expr, $kind:ident, $flag:ident) => {{
                let autodiscover = match original_package
                    .and_then(|package| package.get(package::Field::$flag))
                {
                    Some(package::Value::Local(primitive::Value::Boolean(value))) => Some(*value),
                    None => embedded.then_some(false),
                    _ => return session.fail(Error::State),
                };
                match targets::group(
                    $input,
                    resolve::Kind::$kind,
                    root,
                    name,
                    edition,
                    autodiscover,
                    self.library.is_some(),
                    &mut preparing.configured.warnings,
                    errors,
                    session,
                )? {
                    Ok(group) => group,
                    Err(error) => return Ok(Err(error)),
                }
            }};
        }
        let bins = group!(bins, Bin, Autobins);
        let examples = group!(
            targets::Input::Declared(declared(original, manifest::Field::Example, session)?),
            Example,
            Autoexamples
        );
        let tests = group!(
            targets::Input::Declared(declared(original, manifest::Field::Test, session)?),
            Test,
            Autotests
        );
        let benches = group!(
            targets::Input::Declared(declared(original, manifest::Field::Bench, session)?),
            Bench,
            Autobenches
        );
        session.items(1)?;
        Ok(Ok(Complete {
            library: self,
            groups: Some(Groups {
                bins,
                examples,
                tests,
                benches,
            }),
        }))
    }
}

fn declared<'a, 'd>(
    original: &'a manifest::Original<'d>,
    field: manifest::Field,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Option<&'a manifest::Targets<'d>>> {
    session.check()?;
    match original.get(field) {
        Some(manifest::Value::Targets(targets)) => Ok(Some(targets)),
        None => Ok(None),
        _ => session.fail(Error::State),
    }
}
