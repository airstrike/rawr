//! Shared Option<Value<String>> lookup over the caller's reached config view.

use super::super::super::super::{session::Session, Diagnostic, Step, Tag};
use super::{origin, Definition, Found, Key};
use crate::input::membership::host;
use rawr_toml::document;

/// Complete text and decoded definition retain their admitted owners.
pub(in super::super) struct Value<'a> {
    pub(super) text: &'a str,
    pub(super) origin: origin::Origin<'a>,
}

impl Value<'_> {
    pub(in super::super) fn text(&self) -> &str {
        self.text
    }
    pub(in super::super) fn definition(&self) -> Definition<'_> {
        self.origin.definition()
    }
}

/// The callback reaches values() only where the pinned getter requests get_cv.
pub(in super::super) fn get<'r, 'image: 'r>(
    key: &'r Key<'_>,
    session: &mut Session<'_, host::Host<'image, '_>>,
    mut cv: impl FnMut(&Key<'_>, &mut Session<'_, host::Host<'image, '_>>) -> Step<Option<Found<'r>>>,
) -> Step<Option<Value<'r>>> {
    // Deserialize Option: raw environment presence includes non-UTF-8.
    if !key.has_environment(session)? && cv(key, session)?.is_none() {
        return Ok(None);
    }

    // ValueSource chooses provenance before deserializing the String. It
    // uses presence, not UTF-8 validity, and chooses cv on equal priority.
    session.items(1)?;
    let environment = Definition::Environment(key.environment());
    let present = session.environment(key.environment().as_bytes())?.is_some();
    let origin = match cv(key, session)? {
        Some(value) if !present || !environment.is_higher_priority(&value.definition) => {
            value.definition
        }
        _ => environment,
    };

    // get_string_priv performs get_cv first, then the environment read,
    // then rejects a wrong final type even if the environment has a string.
    let configured = cv(key, session)?;
    let bytes = session.environment(key.environment().as_bytes())?;
    let text = bytes.and_then(|bytes| std::str::from_utf8(bytes).ok());
    if let Some(text) = text {
        session.string(text.len())?;
        session.items(1)?;
    }
    let text = match (configured, text) {
        (Some(value), environment_text) => {
            let Some(text) = value.text else {
                return session.reject(Diagnostic {
                    tag: Tag::ConfigLookup,
                    span: value.span,
                    expected: Some("a string"),
                });
            };
            if value.definition.is_higher_priority(&environment) {
                Some(text)
            } else {
                environment_text.or(Some(text))
            }
        }
        (None, environment_text) => environment_text,
    };
    let Some(text) = text else {
        return session.reject(Diagnostic::at(Tag::ConfigMissing, None));
    };
    // The Value<T> definition codec runs only after String succeeds.
    let origin = origin::Origin::decode(origin, session)?;
    Ok(Some(Value { text, origin }))
}

#[cfg(test)]
mod tests;
