// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../../../path/LICENSE-MIT and ../../../../path/LICENSE-APACHE.
//! FeatureName validation after implicit optional features have been inserted.
use crate::input::membership::manifest::{load::formatted, scalar, session::Session, Error, Step};
use rawr_toml::document;

pub(super) fn validate<A: document::Admission>(
    name: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.string(name.len())?;
    session.items(name.len())?;
    let Err(error) = scalar::Name::parse(name, scalar::Kind::Feature) else {
        return Ok(Ok(()));
    };
    let scalar::Error::Name { reason, .. } = error else {
        return session.fail(Error::State);
    };
    use scalar::NameRejection::*;
    if reason == Empty {
        return formatted(format_args!("feature name cannot be empty"), session).map(Err);
    }
    // Cargo retains the offending name in the typed error before its Display.
    let owned = formatted(format_args!("{name}"), session)?;
    if reason == FeatureDependencyPrefix {
        return formatted(
            format_args!("feature named `{owned}` is not allowed to start with `dep:`"),
            session,
        )
        .map(Err);
    }
    let (character, reason) = match reason {
        FeatureSlash => ('/', "feature name is not allowed to contain slashes"),
        Start(ch) => (ch, "the first character must be a Unicode XID start character or digit (most letters or `_` or `0` to `9`)"),
        Continue(ch) => (ch, "characters must be Unicode XID characters, '-', `+`, or `.` (numbers, `+`, `-`, `_`, `.`, or most letters)"),
        _ => return session.fail(Error::State),
    };
    formatted(
        format_args!("invalid character `{character}` in feature name: `{owned}`, {reason}"),
        session,
    )
    .map(Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;
    #[test]
    fn implicit_feature_names_preserve_cargo_diagnostics() {
        for (name, expected) in [
            ("", "feature name cannot be empty"),
            ("dep:x/", "feature named `dep:x/` is not allowed to start with `dep:`"),
            ("x/y", "invalid character `/` in feature name: `x/y`, feature name is not allowed to contain slashes"),
            ("-x", "invalid character `-` in feature name: `-x`, the first character must be a Unicode XID start character or digit (most letters or `_` or `0` to `9`)"),
            ("x::y", "invalid character `:` in feature name: `x::y`, characters must be Unicode XID characters, '-', `+`, or `.` (numbers, `+`, `-`, `_`, `.`, or most letters)"),
        ] {
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            assert_eq!(validate(name, &mut session).unwrap().unwrap_err(), expected);
        }
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        assert!(validate("1_α+.-", &mut session).unwrap().is_ok());
    }
}
