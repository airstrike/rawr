//! Summary construction checks; feature-map construction follows dependency checks.
pub(super) mod map;
pub(super) mod value;
use super::dependency;
use crate::input::membership::manifest::{
    load::normalize::{dependencies::Kind, target},
    session::Session,
    Step,
};
use rawr_toml::document;

/// Owned feature map that has passed all Summary feature checks.
pub(in super::super) struct Features {
    entries: crate::input::membership::manifest::storage::Buffer<map::Entry>,
}
impl Features {
    pub(in super::super) fn entries(&self) -> &[map::Entry] {
        self.entries.as_slice()
    }
    pub(in super::super) fn warn<A: document::Admission>(
        &self,
        warnings: &mut crate::input::membership::manifest::load::Warnings,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        for entry in self.entries() {
            session.items(1)?;
            session.items(entry.name.len())?;
            if entry.name == "default-features" {
                warnings.push(&["`[features]` defines a feature named `default-features`\nnote: only a feature named `default` will be enabled by default"], session)?;
                break;
            }
        }
        Ok(())
    }
}
pub(in super::super) struct Missing {
    pub(in super::super) feature: String,
    pub(in super::super) value: value::Value,
    pub(in super::super) dependency: String,
    pub(in super::super) weak: bool,
}
pub(in super::super) enum Failure {
    Message(String),
    Missing(Missing),
}
pub(in super::super) fn features<A: document::Admission>(
    features: Option<&crate::input::membership::manifest::schema::manifest::Features<'_>>,
    dependencies: &[dependency::Converted<'_, '_>],
    session: &mut Session<'_, A>,
) -> Step<Result<Features, Failure>> {
    use crate::input::membership::manifest::load::formatted;
    let map = map::Map::build(features, dependencies, session)?;
    match map.validate(session)? {
        Ok(()) => Ok(Ok(Features {
            entries: map.into_entries(),
        })),
        Err(map::validate::Failure::Message(message)) => Ok(Err(Failure::Message(message))),
        Err(map::validate::Failure::Missing {
            feature,
            value,
            dependency,
            weak,
        }) => {
            let feature = formatted(format_args!("{feature}"), session)?;
            let value = value.retain(session)?;
            let dependency = formatted(format_args!("{dependency}"), session)?;
            Ok(Err(Failure::Missing(Missing {
                feature,
                value,
                dependency,
                weak,
            })))
        }
    }
}

pub(in super::super) fn dependencies<A: document::Admission>(
    dependencies: &[dependency::Converted<'_, '_>],
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    for dependency in dependencies {
        session.items(1)?;
        let fields = dependency.fields();
        if fields.optional() && matches!(fields.kind(), Kind::Development) {
            let name = fields.rename().unwrap_or(fields.name());
            return target::formatted(
                format_args!("dev-dependencies are not allowed to be optional: `{name}`"),
                session,
            )
            .map(Err);
        }
    }
    Ok(Ok(()))
}
