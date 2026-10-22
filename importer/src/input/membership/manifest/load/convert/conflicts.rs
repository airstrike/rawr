// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Pinned path-conflict grouping and warning order, after no-target rejection.

use super::super::{
    super::{session::Session, storage, Error, Step},
    config::PathTable,
    normalize::target,
    path, Warnings,
};
use super::{build, target::configure::Kind, Targets};
use rawr_toml::document;
use std::path::Path;

pub(in super::super) fn inspect<A: document::Admission>(
    targets: &Targets<'_, '_>,
    root: &Path,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let ordinary = targets.ordinary.as_slice().iter().map(|target| Input {
        path: target.path(),
        name: target.name(),
        description: match target.kind() {
            Kind::Lib => "lib",
            Kind::Bin => "bin",
            Kind::Example => "example",
            Kind::Test => "integration-test",
            Kind::Bench => "bench",
            Kind::CustomBuild | Kind::Metabuild => "build-script",
        },
    });
    let builds = targets
        .builds
        .as_slice()
        .iter()
        .filter_map(|target| match target.source() {
            build::Source::Path(path) => Some(Input {
                path,
                name: target.name(),
                description: "build-script",
            }),
            build::Source::Metabuild => None,
        });
    inspect_inputs(ordinary.chain(builds), root, warnings, session)
}

struct Input<'a> {
    path: &'a Path,
    name: &'a str,
    description: &'static str,
}
struct Member<'a> {
    name: &'a str,
    description: &'static str,
}

fn inspect_inputs<'a, A: document::Admission>(
    inputs: impl Iterator<Item = Input<'a>>,
    root: &Path,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.check()?;
    let mut paths = storage::slots::Slots::new();
    let Some(mut paths) = paths.writer() else {
        return session.fail(Error::State);
    };
    let mut source = PathTable::new();
    let mut groups: storage::Buffer<storage::Buffer<Member<'a>>> = storage::Buffer::new();
    for input in inputs {
        session.items(1)?;
        let full = path::join(root, input.path, session)?;
        let full = paths.push(full, session)?;
        let index = match source.merge(full.as_path(), groups.len(), session)? {
            Some(index) => index,
            None => groups.push(storage::Buffer::new(), session)?,
        };
        let Some(group) = groups.as_mut_slice().get_mut(index) else {
            return session.fail(Error::State);
        };
        group.push(
            Member {
                name: input.name,
                description: input.description,
            },
            session,
        )?;
    }
    // A filtered IntoIter has lower size hint zero. Cargo's collect therefore
    // starts empty and performs ordinary insertion for each surviving group.
    let mut conflicts = PathTable::new();
    let mut source = source.into_iter(session)?;
    while let Some((path, index)) = source.next(session)? {
        let Some(group) = groups.as_slice().get(index) else {
            return session.fail(Error::State);
        };
        if group.len() > 1 {
            conflicts.insert(path, index, session)?;
        }
    }
    struct Members<'a, 'b>(&'a [Member<'b>]);
    impl std::fmt::Display for Members<'_, '_> {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for (index, member) in self.0.iter().enumerate() {
                if index != 0 {
                    output.write_str("\n")?;
                }
                write!(
                    output,
                    "  * `{}` target `{}`",
                    member.description, member.name
                )?;
            }
            Ok(())
        }
    }
    let mut conflicts = conflicts.iter(session)?;
    while let Some((path, index)) = conflicts.next(session)? {
        let Some(group) = groups.as_slice().get(index) else {
            return session.fail(Error::State);
        };
        let warning = target::formatted(
            format_args!(
                "file `{}` found to be present in multiple build targets:\n{}",
                path.display(),
                Members(group.as_slice())
            ),
            session,
        )?;
        warnings.push(&[&warning], session)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
