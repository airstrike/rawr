//! Cargo's initial workspace configuration, before cache insertion/normalization.

use std::path::Path;

use rawr_toml::document;

use super::super::{
    schema::{dependency, manifest, package, primitive, workspace},
    session, value, Diagnostic, Error, Step, Tag,
};
use super::{lints, Warnings};

/// Borrowed unresolved configuration. A root is a declaration, not a validated workspace.
pub(super) enum Configuration<'a, 'd> {
    Root {
        directory: &'a Path,
        declaration: Option<&'a workspace::Workspace<'d>>,
    },
    Member {
        root: Option<value::Text<'d>>,
    },
}

impl Configuration<'_, '_> {
    /// Cargo's get_ws_root selection only. The caller must finish loading the
    /// candidate first; this pathname does not confer loaded-root authority.
    pub(super) fn selected_root<A: document::Admission>(
        &self,
        manifest: &Path,
        look_from: &Path,
        session: &mut session::Session<'_, A>,
    ) -> Step<Option<std::path::PathBuf>> {
        session.check()?;
        match self {
            Self::Root {
                directory,
                declaration,
            } => {
                let root = super::roots::Root {
                    directory,
                    declaration: *declaration,
                };
                if root.excluded(look_from, session)? {
                    Ok(None)
                } else {
                    super::path::copy(manifest, session).map(Some)
                }
            }
            Self::Member { root } => super::driver::Request {
                manifest,
                explicit_root: root.as_ref().map(|root| root.as_str()),
            }
            .explicit_candidate(session),
        }
    }
}

pub(super) fn configure<'a, 'd, A: document::Admission>(
    original: &'a manifest::Original<'d>,
    manifest_file: &'a Path,
    embedded: bool,
    context: lints::Context,
    warnings: &mut Warnings,
    session: &mut session::Session<'_, A>,
) -> Step<Configuration<'a, 'd>> {
    session.check()?;
    if embedded {
        return root(None, manifest_file, session);
    }
    let declaration = match original.get(manifest::Field::Workspace) {
        Some(manifest::Value::Workspace(workspace)) => Some(workspace),
        None => None,
        _ => return session.fail(Error::State),
    };
    // The schema accessor prefers package, but both aliases were already converted.
    let package = original
        .get(manifest::Field::Package)
        .or_else(|| original.get(manifest::Field::Project));
    let pointer = match package {
        Some(manifest::Value::Package(package)) => match package.get(package::Field::Workspace) {
            Some(package::Value::Local(primitive::Value::String(text))) => Some(*text),
            None => None,
            _ => return session.fail(Error::State),
        },
        None => None,
        _ => return session.fail(Error::State),
    };
    match (declaration, pointer) {
        (Some(_), Some(_)) => {
            session.reject(Diagnostic::at(Tag::WorkspaceConflict, original.span()))
        }
        (None, root) => {
            session.items(1)?;
            Ok(Configuration::Member { root })
        }
        (Some(declaration), None) => {
            let table = match declaration.get(workspace::Field::Lints) {
                Some(workspace::Value::Lints(table)) => Some(table),
                None => None,
                _ => return session.fail(Error::State),
            };
            lints::verify(table, context, warnings, session)?;
            if let Some(workspace::Value::Dependencies(dependencies)) =
                declaration.get(workspace::Field::Dependencies)
            {
                for (_, _, dep) in dependencies.entries() {
                    if let dependency::direct::Kind::Detailed(details) = dep.kind() {
                        for (field, tag) in [
                            (dependency::Field::Optional, Tag::WorkspaceOptional),
                            (dependency::Field::Public, Tag::WorkspacePublic),
                        ] {
                            if matches!(details.get(field), Some(primitive::Value::Boolean(true))) {
                                return session.reject(Diagnostic::at(tag, dep.span()));
                            }
                        }
                    }
                }
                // Cargo emits extras only after every dependency passes both checks.
                for (name, _, dep) in dependencies.entries() {
                    if let dependency::direct::Kind::Detailed(details) = dep.kind() {
                        for (key, _) in details.extras() {
                            warnings.push(
                                &[
                                    "unused manifest key: workspace.dependencies.",
                                    name.as_str(),
                                    ".",
                                    key.text.as_str(),
                                ],
                                session,
                            )?;
                        }
                    }
                }
            }
            root(Some(declaration), manifest_file, session)
        }
    }
}

fn root<'a, 'd, A: document::Admission>(
    declaration: Option<&'a workspace::Workspace<'d>>,
    manifest_file: &'a Path,
    session: &mut session::Session<'_, A>,
) -> Step<Configuration<'a, 'd>> {
    let Some(directory) = manifest_file.parent() else {
        return session.fail(Error::State);
    };
    session.items(1)?;
    Ok(Configuration::Root {
        directory,
        declaration,
    })
}
