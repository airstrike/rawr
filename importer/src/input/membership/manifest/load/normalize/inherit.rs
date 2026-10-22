// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned workspace dependency inheritance only; see inherit/PROVENANCE.md.
//! This stage grants neither successful manifest loading nor membership.

use std::path::Path;

use rawr_toml::document;

use super::super::super::{
    schema::{
        dependency::{direct, inheritable, Direct, Field, Inheritable, Inherited},
        primitive, workspace,
    },
    session::Session,
    value, Diagnostic, Step, Tag,
};
use super::super::{path, roots, Warnings};

#[cfg(test)]
mod tests;

pub(in super::super) enum Progress<'a, 'd> {
    Direct(View<'a, 'd>),
    Workspace(Workspace<'a, 'd>),
}

/// Retains the exact member and label while the caller performs Cargo's lazy
/// workspace selection. A cached declaration is not a successfully loaded root.
pub(in super::super) struct Workspace<'a, 'd> {
    name: &'a str,
    member: &'a Inherited<'d>,
    package_root: &'a Path,
    edition: u16,
}

pub(in super::super) struct View<'a, 'd> {
    original: &'a Direct<'d>,
    inherited: Option<&'a Inherited<'d>>,
    path: Option<String>,
    enable_defaults: bool,
}

pub(in super::super) enum Value<'a, 'd> {
    Original(&'a primitive::Value<'d>),
    Text(&'a str),
    Boolean(bool),
    Features(Features<'a, 'd>),
}

/// Keeps presence, ordering and duplicates without cloning admitted strings.
pub(in super::super) struct Features<'a, 'd> {
    workspace: &'a [value::Text<'d>],
    member: &'a [value::Text<'d>],
}
impl<'a, 'd> Features<'a, 'd> {
    pub(in super::super) fn iter(&self) -> impl Iterator<Item = &str> {
        self.workspace
            .iter()
            .chain(self.member)
            .map(|text| text.as_str())
    }
}

pub(in super::super) fn dependency<'a, 'd, A: document::Admission>(
    name: &'a str,
    input: &'a Inheritable<'d>,
    package_root: &'a Path,
    edition: u16,
    session: &mut Session<'_, A>,
) -> Step<Progress<'a, 'd>> {
    session.check()?;
    session.items(1)?;
    Ok(match input.kind() {
        inheritable::Kind::Value(original) => Progress::Direct(View {
            original,
            inherited: None,
            path: None,
            enable_defaults: false,
        }),
        inheritable::Kind::Inherit(member) => Progress::Workspace(Workspace {
            name,
            member,
            package_root,
            edition,
        }),
    })
}

impl<'a, 'd> Workspace<'a, 'd> {
    pub(in super::super) fn resume_declared<A: document::Admission>(
        self,
        root: roots::Root<'a, 'd>,
        warnings: &mut Warnings,
        session: &mut Session<'_, A>,
    ) -> Step<View<'a, 'd>> {
        session.check()?;
        let Some(workspace::Value::Dependencies(dependencies)) = root
            .declaration
            .and_then(|root| root.get(workspace::Field::Dependencies))
        else {
            return session.reject(Diagnostic::at(
                Tag::DependencyWorkspaceMissing,
                self.member.span(),
            ));
        };
        let mut selected = None;
        for (name, _, dependency) in dependencies.entries() {
            session.items(1)?;
            if name.as_str() == self.name {
                selected = Some(dependency);
                break;
            }
        }
        let Some(original) = selected else {
            return session.reject(Diagnostic::at(
                Tag::DependencyInheritedMissing,
                self.member.span(),
            ));
        };
        let mut view = View {
            original,
            inherited: Some(self.member),
            path: None,
            enable_defaults: false,
        };
        // get_dependency rebases before merging defaults/features, except when
        // path-base processing owns the path in the later normalization stage.
        if original_field(original, Field::Base).is_none()
            && let Some(primitive::Value::String(relative)) = original_field(original, Field::Path)
        {
            view.path = Some(
                match path::rebase(
                    root.directory,
                    self.package_root,
                    relative.as_str(),
                    session,
                )? {
                    Ok(path) => path,
                    Err(path::Invalid::Relative) => {
                        return session.reject(Diagnostic::at(
                            Tag::DependencyPathRelative,
                            self.member.span(),
                        ))
                    }
                    Err(path::Invalid::Utf8) => {
                        return session
                            .reject(Diagnostic::at(Tag::DependencyPathUtf8, self.member.span()))
                    }
                },
            );
        }
        let member = boolean(self.member.get(Field::DefaultFeatures))
            .or_else(|| boolean(self.member.get(Field::DefaultFeatures2)));
        let workspace = boolean(original_field(original, Field::DefaultFeatures))
            .or_else(|| boolean(original_field(original, Field::DefaultFeatures2)));
        match (member, workspace) {
            (Some(true), Some(false)) => view.enable_defaults = true,
            (Some(false), Some(true) | None) => {
                if self.edition >= 2024 {
                    return session.reject(Diagnostic::at(
                        Tag::DependencyDefaultFeatures,
                        self.member.span(),
                    ));
                }
                warnings.push(
                    &[
                        "`default-features` is ignored for ",
                        self.name,
                        ", since `default-features` was ",
                        if workspace.is_some() {
                            "true"
                        } else {
                            "not specified"
                        },
                        " for `workspace.dependencies.",
                        self.name,
                        "`, this could become a hard error in the future",
                    ],
                    session,
                )?;
            }
            _ => {}
        }
        session.items(1)?;
        Ok(view)
    }
}

impl<'a, 'd> View<'a, 'd> {
    pub(in super::super) fn simple(&self) -> Option<&str> {
        match (self.inherited, self.original.kind()) {
            (None, direct::Kind::Simple(version)) => Some(version.as_str()),
            _ => None,
        }
    }
    pub(in super::super) fn get(&self, field: Field) -> Option<Value<'_, 'd>> {
        if let Some(member) = self.inherited {
            match field {
                Field::Optional | Field::Public => return member.get(field).map(Value::Original),
                Field::DefaultFeatures if self.enable_defaults => {
                    return Some(Value::Boolean(true))
                }
                Field::Features => {
                    let workspace = strings(original_field(self.original, field));
                    let member = strings(member.get(field));
                    return (workspace.is_some() || member.is_some()).then(|| {
                        Value::Features(Features {
                            workspace: workspace.unwrap_or(&[]),
                            member: member.unwrap_or(&[]),
                        })
                    });
                }
                Field::Path if self.path.is_some() => return self.path.as_deref().map(Value::Text),
                Field::Version => {
                    if let direct::Kind::Simple(version) = self.original.kind() {
                        return Some(Value::Text(version.as_str()));
                    }
                }
                _ => {}
            }
        }
        original_field(self.original, field).map(Value::Original)
    }
    pub(in super::super) fn extras(
        &self,
    ) -> impl Iterator<Item = (&value::Key<'d>, &value::Projection<'d>)> {
        let detailed = match self.original.kind() {
            direct::Kind::Detailed(detailed) => Some(detailed),
            direct::Kind::Simple(_) => None,
        };
        detailed.into_iter().flat_map(|detailed| detailed.extras())
    }
}

fn original_field<'a, 'd>(
    original: &'a Direct<'d>,
    field: Field,
) -> Option<&'a primitive::Value<'d>> {
    match original.kind() {
        direct::Kind::Detailed(detailed) => detailed.get(field),
        direct::Kind::Simple(_) => None,
    }
}
fn boolean(value: Option<&primitive::Value<'_>>) -> Option<bool> {
    match value {
        Some(primitive::Value::Boolean(value)) => Some(*value),
        _ => None,
    }
}
fn strings<'a, 'd>(value: Option<&'a primitive::Value<'d>>) -> Option<&'a [value::Text<'d>]> {
    match value {
        Some(primitive::Value::Strings(value)) => Some(value.as_slice()),
        _ => None,
    }
}
