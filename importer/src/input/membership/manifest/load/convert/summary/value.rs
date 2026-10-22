// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Cargo summary.rs FeatureValue::new; syntax validity is checked later.
use crate::input::membership::manifest::{load::formatted, session::Session, Step};
use rawr_toml::document;

pub(in super::super::super) enum Value {
    Feature(String),
    Dependency(String),
    DependencyFeature {
        name: String,
        feature: String,
        weak: bool,
    },
}
impl Value {
    pub(in super::super::super) fn retain<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        Ok(match self {
            Self::Feature(name) => Self::Feature(formatted(format_args!("{name}"), session)?),
            Self::Dependency(name) => Self::Dependency(formatted(format_args!("{name}"), session)?),
            Self::DependencyFeature {
                name,
                feature,
                weak,
            } => Self::DependencyFeature {
                name: formatted(format_args!("{name}"), session)?,
                feature: formatted(format_args!("{feature}"), session)?,
                weak: *weak,
            },
        })
    }
    pub(in super::super::super) fn parse<A: document::Admission>(
        text: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.string(text.len())?;
        session.items(text.len())?;
        match text.split_once('/') {
            Some((dependency, feature)) => {
                let stripped = dependency.strip_suffix('?');
                let name = stripped.unwrap_or(dependency);
                let name = formatted(format_args!("{name}"), session)?;
                let feature = formatted(format_args!("{feature}"), session)?;
                Ok(Self::DependencyFeature {
                    name,
                    feature,
                    weak: stripped.is_some(),
                })
            }
            None => match text.strip_prefix("dep:") {
                Some(name) => Ok(Self::Dependency(formatted(
                    format_args!("{name}"),
                    session,
                )?)),
                None => Ok(Self::Feature(formatted(format_args!("{text}"), session)?)),
            },
        }
    }
    pub(in super::super::super) fn explicit_dependency(&self) -> Option<&str> {
        match self {
            Self::Dependency(name) => Some(name),
            _ => None,
        }
    }
    pub(in super::super::super) fn name(&self) -> &str {
        match self {
            Self::Feature(name) | Self::Dependency(name) | Self::DependencyFeature { name, .. } => {
                name
            }
        }
    }
}
impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Feature(name) => write!(formatter, "{name}"),
            Self::Dependency(name) => write!(formatter, "dep:{name}"),
            Self::DependencyFeature {
                name,
                feature,
                weak,
            } => write!(
                formatter,
                "{name}{}/{feature}",
                if *weak { "?" } else { "" }
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;
    #[test]
    fn feature_values_split_slash_before_dependency_prefix() {
        for (text, name, explicit, weak) in [
            ("plain", "plain", None, None),
            ("dep:foo", "foo", Some("foo"), None),
            ("foo/bar", "foo", None, Some(false)),
            ("foo?/bar", "foo", None, Some(true)),
            ("dep:foo?/bar/baz", "dep:foo", None, Some(true)),
            ("foo??/", "foo?", None, Some(true)),
            ("dep:", "", Some(""), None),
            ("/", "", None, Some(false)),
        ] {
            let owned = text.to_owned();
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let parsed = Value::parse(&owned, &mut session).unwrap();
            drop(owned);
            assert_eq!(parsed.name(), name);
            assert_eq!(parsed.explicit_dependency(), explicit);
            assert_eq!(
                match &parsed {
                    Value::DependencyFeature { weak, .. } => Some(*weak),
                    _ => None,
                },
                weak
            );
            assert_eq!(parsed.to_string(), text);
        }
    }
}
