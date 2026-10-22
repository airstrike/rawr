// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Explicit resolver selection, util/toml/mod.rs:1500–1512.

use super::super::{
    super::{session::Session, Step},
    normalize::target,
};
use rawr_toml::document;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super) enum Behavior {
    V1,
    V2,
    V3,
}

pub(in super::super) fn select<A: document::Admission>(
    package: Option<&str>,
    workspace: Option<&str>,
    session: &mut Session<'_, A>,
) -> Step<Result<Option<Behavior>, String>> {
    session.check()?;
    session.items(1)?;
    let selected = match (package, workspace) {
        (None, None) => return Ok(Ok(None)),
        (Some(_), Some(_)) => {
            return target::formatted(
                format_args!(
                    "cannot specify `resolver` field in both `[workspace]` and `[package]`"
                ),
                session,
            )
            .map(Err)
        }
        (Some(value), None) | (None, Some(value)) => value,
    };
    let behavior = match selected {
        "1" => Behavior::V1,
        "2" => Behavior::V2,
        "3" => Behavior::V3,
        other => return target::formatted(format_args!("`resolver` setting `{other}` is not valid, valid options are \"1\", \"2\" or \"3\""), session).map(Err),
    };
    Ok(Ok(Some(behavior)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;

    #[test]
    fn selection_preserves_absence_and_rejects_conflict_before_parsing() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        assert_eq!(select(None, None, &mut session).unwrap().unwrap(), None);
        for (text, behavior) in [
            ("1", Behavior::V1),
            ("2", Behavior::V2),
            ("3", Behavior::V3),
        ] {
            assert_eq!(
                select(Some(text), None, &mut session).unwrap().unwrap(),
                Some(behavior)
            );
            assert_eq!(
                select(None, Some(text), &mut session).unwrap().unwrap(),
                Some(behavior)
            );
        }
        assert_eq!(
            select(Some("bad"), Some("bad"), &mut session)
                .unwrap()
                .unwrap_err(),
            "cannot specify `resolver` field in both `[workspace]` and `[package]`"
        );
        for invalid in ["", " 2", "02", "4"] {
            assert_eq!(select(Some(invalid), None, &mut session).unwrap().unwrap_err(), format!("`resolver` setting `{invalid}` is not valid, valid options are \"1\", \"2\" or \"3\""));
        }
    }
}
