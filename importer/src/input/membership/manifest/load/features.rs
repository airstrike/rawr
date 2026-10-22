//! Pinned Cargo's post-Original feature context, without ambient configuration.

mod required;
pub(super) use required::append;
mod table;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use super::super::{
    schema::{manifest, primitive},
    session, value, Diagnostic, Error, Step, Tag,
};
use super::Warnings;

pub(in super::super) use table::Feature;

/// Already associated, admitted Cargo values. This module neither acquires nor
/// proves them; the allow-list is borrowed from the closed configuration owner.
#[derive(Clone, Copy)]
pub(in super::super) struct Context<'a> {
    pub(in super::super) allow_features: Option<&'a [&'a str]>,
    pub(in super::super) nightly_features_allowed: bool,
    pub(in super::super) channel: &'a str,
    pub(in super::super) is_local: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Advice {
    AddDeclaration,
    NewerNightly,
    NewerCargo,
}

/// Fixed diagnostic data; indices refer to the completed Original feature list.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Invalid {
    Unknown {
        index: usize,
    },
    UnknownUnderscore {
        index: usize,
    },
    UnknownCli {
        index: usize,
    },
    Duplicate {
        index: usize,
        feature: Feature,
    },
    Channel {
        index: usize,
        feature: Feature,
    },
    Denied {
        index: usize,
        feature: Feature,
    },
    Removed {
        index: usize,
        feature: Feature,
        local: bool,
    },
    Required {
        feature: Feature,
        advice: Advice,
        hint: bool,
    },
}

/// Checked activation data borrowing the complete, already converted declaration.
/// Stable enablement is distinct from the explicit bits used to detect duplicates.
pub(in super::super) struct Features<'a, 'd> {
    activated: &'a [value::Text<'d>],
    explicit: [bool; 27],
    nightly_features_allowed: bool,
    is_local: bool,
}

impl<'a, 'd> Features<'a, 'd> {
    pub(in super::super) fn interpret<A: document::Admission>(
        original: &'a manifest::Original<'d>,
        context: Context<'_>,
        warnings: &mut Warnings,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let activated = match original.get(manifest::Field::CargoFeatures) {
            Some(manifest::Value::Primitive(primitive::Value::Strings(values))) => {
                values.as_slice()
            }
            None => &[],
            _ => return session.fail(Error::State),
        };
        let mut explicit = [false; 27];
        for (index, name) in activated.iter().enumerate() {
            let name = name.as_str();
            session.string(name.len())?;
            let Some(feature) = table::find(name) else {
                let invalid = if name.contains('_') {
                    Invalid::UnknownUnderscore { index }
                } else if table::is_cli(name) {
                    Invalid::UnknownCli { index }
                } else {
                    Invalid::Unknown { index }
                };
                return reject(invalid, session);
            };
            if explicit[feature as usize] {
                return reject(Invalid::Duplicate { index, feature }, session);
            }
            let metadata = feature.metadata();
            match metadata.status {
                table::Status::Stable => {
                    // Cargo consults diagnostic channel here, separately from the
                    // context's previously captured nightly eligibility.
                    session.string(context.channel.len())?;
                    let channel = match context.channel {
                        "dev" | "nightly" => "nightly/",
                        "beta" => "beta/",
                        _ => "",
                    };
                    warnings.push(&[
                        "the cargo feature `", name, "` has been stabilized in the ",
                        metadata.version,
                        " release and is no longer necessary to be listed in the manifest\n  See https://doc.rust-lang.org/",
                        channel, "cargo/", metadata.docs,
                        " for more information about using this feature.",
                    ], session)?;
                }
                table::Status::Unstable if !context.nightly_features_allowed => {
                    return reject(Invalid::Channel { index, feature }, session);
                }
                table::Status::Unstable => {
                    if let Some(allow) = context.allow_features
                        && !allow.contains(&name)
                    {
                        return reject(Invalid::Denied { index, feature }, session);
                    }
                }
                table::Status::Removed => {
                    return reject(
                        Invalid::Removed {
                            index,
                            feature,
                            local: context.is_local,
                        },
                        session,
                    );
                }
            }
            explicit[feature as usize] = true;
        }
        session.items(1)?;
        Ok(Self {
            activated,
            explicit,
            nightly_features_allowed: context.nightly_features_allowed,
            is_local: context.is_local,
        })
    }

    pub(in super::super) fn activated<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<&'a [value::Text<'d>]> {
        session.check()?;
        Ok(self.activated)
    }

    pub(in super::super) fn is_enabled<A: document::Admission>(
        &self,
        feature: Feature,
        session: &mut session::Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        Ok(feature.metadata().status == table::Status::Stable || self.explicit[feature as usize])
    }

    pub(in super::super) fn require<A: document::Admission>(
        &self,
        feature: Feature,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.require_with_hint(feature, None, session)
    }

    /// The caller retains the optional diagnostic hint. Only its presence is
    /// recorded in the bounded category; no owned version/channel/hint is rendered.
    pub(in super::super) fn require_with_hint<A: document::Admission>(
        &self,
        feature: Feature,
        hint: Option<&str>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        if self.is_enabled(feature, session)? {
            return Ok(());
        }
        let advice = match (self.nightly_features_allowed, self.is_local) {
            (true, true) => Advice::AddDeclaration,
            (true, false) => Advice::NewerNightly,
            (false, _) => Advice::NewerCargo,
        };
        reject(
            Invalid::Required {
                feature,
                advice,
                hint: hint.is_some(),
            },
            session,
        )
    }
}

fn reject<T, A: document::Admission>(
    invalid: Invalid,
    session: &mut session::Session<'_, A>,
) -> Step<T> {
    // Upstream anyhow errors carry no TOML span at this post-typed stage.
    session.reject(Diagnostic::at(Tag::Feature(invalid), None))
}
