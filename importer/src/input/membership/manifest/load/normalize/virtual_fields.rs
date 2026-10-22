// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Virtual normalization's ordered requires_package check, not conversion.

use super::super::super::{
    schema::manifest::{Field, Original},
    session::Session,
    Step,
};
use super::target;
use rawr_toml::document;

pub(in super::super) fn check<A: document::Admission>(
    original: &Original<'_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    // Cargo builds this fixed array eagerly, then takes its first present item.
    // Presence of an empty collection still requires a package.
    session.items(15)?;
    let fields = [
        (original.get(Field::Badges).is_some(), "badges"),
        (original.get(Field::Features).is_some(), "features"),
        (original.get(Field::Lib).is_some(), "lib"),
        (original.get(Field::Bin).is_some(), "bin"),
        (original.get(Field::Example).is_some(), "example"),
        (original.get(Field::Test).is_some(), "test"),
        (original.get(Field::Bench).is_some(), "bench"),
        (original.get(Field::Dependencies).is_some(), "dependencies"),
        (
            original
                .get(Field::DevDependencies)
                .or(original.get(Field::DevDependencies2))
                .is_some(),
            "dev-dependencies",
        ),
        (
            original
                .get(Field::BuildDependencies)
                .or(original.get(Field::BuildDependencies2))
                .is_some(),
            "build-dependencies",
        ),
        (original.get(Field::Target).is_some(), "target"),
        (original.get(Field::Lints).is_some(), "lints"),
        (original.get(Field::Hints).is_some(), "hints"),
    ];
    for (present, field) in fields {
        session.items(1)?;
        if present {
            return target::formatted(
                format_args!(
                    "this virtual manifest specifies a `{field}` section, which is not allowed"
                ),
                session,
            )
            .map(Err);
        }
    }
    Ok(Ok(()))
}

#[cfg(test)]
mod tests;
