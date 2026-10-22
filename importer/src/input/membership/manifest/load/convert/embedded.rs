// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Original-field embedded restrictions, after the conversion preamble.

use super::super::{
    super::{
        schema::{
            manifest::{Field, Original, Value},
            package,
        },
        session::Session,
        storage, Error, Step,
    },
    normalize::target,
};
use rawr_toml::document;

pub(in super::super) fn check<A: document::Admission>(
    original: &Original<'_>,
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    let mut invalid = storage::Buffer::new();
    for (field, name) in [
        (Field::Workspace, "`workspace`"),
        (Field::Lib, "`lib`"),
        (Field::Bin, "`bin`"),
        (Field::Example, "`example`"),
        (Field::Test, "`test`"),
        (Field::Bench, "`bench`"),
        (Field::BuildDependencies, "`build-dependencies`"),
        (Field::BuildDependencies2, "`build_dependencies`"),
    ] {
        session.items(1)?;
        if original.get(field).is_some() {
            invalid.push(name, session)?;
        }
    }
    session.items(2)?;
    match original
        .get(Field::Package)
        .or(original.get(Field::Project))
    {
        Some(Value::Package(package)) => {
            for (field, name) in [
                (package::Field::Workspace, "`package.workspace`"),
                (package::Field::Build, "`package.build`"),
                (package::Field::Metabuild, "`package.metabuild`"),
                (package::Field::Links, "`package.links`"),
                (package::Field::Autolib, "`package.autolib`"),
                (package::Field::Autobins, "`package.autobins`"),
                (package::Field::Autoexamples, "`package.autoexamples`"),
                (package::Field::Autotests, "`package.autotests`"),
                (package::Field::Autobenches, "`package.autobenches`"),
                (package::Field::DefaultRun, "`package.default-run`"),
            ] {
                session.items(1)?;
                if package.get(field).is_some() {
                    invalid.push(name, session)?;
                }
            }
        }
        None => {}
        _ => return session.fail(Error::State),
    }
    if invalid.len() == 0 {
        return Ok(Ok(()));
    }
    struct Names<'a>(&'a [&'static str]);
    impl std::fmt::Display for Names<'_> {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for (index, name) in self.0.iter().enumerate() {
                if index != 0 {
                    output.write_str(", ")?;
                }
                output.write_str(name)?;
            }
            Ok(())
        }
    }
    let verb = if invalid.len() == 1 { "is" } else { "are" };
    target::formatted(
        format_args!(
            "{} {verb} not allowed in embedded manifests",
            Names(invalid.as_slice())
        ),
        session,
    )
    .map(Err)
}

#[cfg(test)]
mod tests;
