//! Retained dependency fields before artifact conversion; no complete authority.

use super::super::{
    platform::Platform,
    source::{Id, Owner},
    url,
};
use super::{dependencies, inherit, primitive, source, target, text, Error, Field, Session, Step};
use crate::input::membership::manifest::{load::config, scalar::requirement::Requirement, storage};
use crate::{cargo, input::membership::host};
use rawr_toml::document;

/// Artifact conversion must consume this prefix before any complete dependency.
pub(in super::super::super) struct BeforeArtifact<'p, 's> {
    name: String,
    requirement: Option<Requirement>,
    source: Id<'s>,
    registry: Option<Id<'s>>,
    features: storage::Buffer<String>,
    defaults: bool,
    optional: bool,
    platform: Option<Platform<'p>>,
    kind: dependencies::Kind,
    rename: Option<String>,
    public: bool,
    promoted: Option<String>,
}
impl<'p, 's> BeforeArtifact<'p, 's> {
    #[cfg(test)]
    pub(in super::super::super) fn promoted_version(&self) -> Option<&str> {
        self.promoted.as_deref()
    }
    pub(in super::super::super) fn name(&self) -> &str {
        &self.name
    }
    pub(in super::super::super) fn requirement(&self) -> Option<&Requirement> {
        self.requirement.as_ref()
    }
    pub(in super::super::super) fn source(&self) -> Id<'s> {
        self.source
    }
    pub(in super::super::super) fn registry(&self) -> Option<Id<'s>> {
        self.registry
    }
    pub(in super::super::super) fn features(&self) -> &[String] {
        self.features.as_slice()
    }
    pub(in super::super::super) fn defaults(&self) -> bool {
        self.defaults
    }
    pub(in super::super::super) fn optional(&self) -> bool {
        self.optional
    }
    pub(in super::super::super) fn platform(&self) -> Option<&Platform<'p>> {
        self.platform.as_ref()
    }
    pub(in super::super::super) fn kind(&self) -> dependencies::Kind {
        self.kind
    }
    pub(in super::super::super) fn rename(&self) -> Option<&str> {
        self.rename.as_deref()
    }
    pub(in super::super::super) fn public(&self) -> bool {
        self.public
    }
    pub(super) fn promotion(&mut self, promoted: Option<String>) {
        self.promoted = promoted;
    }
    pub(super) fn replace_requirement(&mut self, requirement: Requirement) {
        self.requirement = Some(requirement);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn retain<'p, 's, 'image: 's>(
    dependency: super::input::Input<'_, '_>,
    version: Option<&str>,
    selected: Id<'s>,
    sources: &mut Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    kind: dependencies::Kind,
    platform: Option<&Platform<'p>>,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<BeforeArtifact<'p, 's>, String>> {
    let package = match dependency.get(Field::Package) {
        None => None,
        Some(inherit::Value::Original(primitive::Value::Name(value))) => Some(value.as_str()),
        _ => return session.fail(Error::State),
    };
    let name = own(package.unwrap_or(dependency.name()), session)?;
    let requirement = match version {
        None => None,
        Some(version) => match Requirement::parse(version, session)? {
            Ok(value) => Some(value),
            Err(error) => return target::formatted(format_args!("failed to parse the version requirement `{version}` for dependency `{name}`: {error}"), session).map(Err),
        },
    };
    // The normalized schema makes Cargo's nonempty-name assertion unreachable.
    if name.is_empty() {
        return session.fail(Error::State);
    }
    session.items(1)?;
    let mut result = BeforeArtifact {
        name,
        requirement,
        source: selected,
        registry: None,
        features: storage::Buffer::new(),
        defaults: true,
        optional: false,
        platform: None,
        kind: dependencies::Kind::Normal,
        rename: None,
        public: false,
        promoted: None,
    };
    match dependency.get(Field::Features) {
        None => {}
        Some(inherit::Value::Original(primitive::Value::Strings(values))) => {
            for value in values.as_slice() {
                push(&mut result.features, value.as_str(), session)?;
            }
        }
        Some(inherit::Value::Features(values)) => {
            for value in values.iter() {
                push(&mut result.features, value, session)?;
            }
        }
        _ => return session.fail(Error::State),
    }
    result.defaults = boolean(dependency.get(Field::DefaultFeatures), session)?
        .or(boolean(dependency.get(Field::DefaultFeatures2), session)?)
        .unwrap_or(true);
    result.optional = boolean(dependency.get(Field::Optional), session)?.unwrap_or(false);
    result.platform = platform.map(|value| value.copy(session)).transpose()?;
    if let Some(value) = dependency.get(Field::Registry) {
        let inherit::Value::Original(primitive::Value::Name(name)) = value else {
            return session.fail(Error::State);
        };
        result.registry = Some(
            match source::named(name.as_str(), sources, config, limits, session)? {
                Ok(id) => id,
                Err(error) => return Ok(Err(error)),
            },
        );
    }
    if let Some(index) = text(dependency.get(Field::RegistryIndex), session)? {
        let parsed = match url::parse(index, session)? {
            Ok(url) => url,
            Err(error) => return Ok(Err(error)),
        };
        result.registry = Some(match sources.registry(&parsed, None, session)? {
            Ok(id) => id,
            Err(error) => return Ok(Err(error)),
        });
    }
    result.kind = kind;
    if package.is_some() {
        result.rename = Some(own(dependency.name(), session)?);
    }
    if let Some(public) = boolean(dependency.get(Field::Public), session)? {
        // The normalized dependency stage removes/rejects non-normal public.
        if public && !matches!(kind, dependencies::Kind::Normal) {
            return session.fail(Error::State);
        }
        result.public = public;
    }
    Ok(Ok(result))
}
fn boolean<A: document::Admission>(
    value: Option<inherit::Value<'_, '_>>,
    session: &mut Session<'_, A>,
) -> Step<Option<bool>> {
    match value {
        None => Ok(None),
        Some(inherit::Value::Boolean(value)) => Ok(Some(value)),
        Some(inherit::Value::Original(primitive::Value::Boolean(value))) => Ok(Some(*value)),
        _ => session.fail(Error::State),
    }
}
fn own<A: document::Admission>(value: &str, session: &mut Session<'_, A>) -> Step<String> {
    target::formatted(format_args!("{value}"), session)
}
fn push<A: document::Admission>(
    values: &mut storage::Buffer<String>,
    value: &str,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    let value = own(value, session)?;
    values.push(value, session)?;
    Ok(())
}
