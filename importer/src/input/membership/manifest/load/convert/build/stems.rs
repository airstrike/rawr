// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Pinned duplicate build-script grouping and missing-stem containment.

use super::super::super::{
    super::{session::Session, storage, Error, Step},
    config::Table,
    normalize::target,
};
use rawr_toml::document;
use std::path::Path;

pub(super) fn check<A: document::Admission>(
    scripts: &[&str],
    session: &mut Session<'_, A>,
) -> Step<Result<(), String>> {
    session.check()?;
    let mut table = Table::new();
    let mut groups: storage::Buffer<storage::Buffer<&str>> = storage::Buffer::new();
    for &script in scripts {
        session.items(1)?;
        session.string(script.len())?;
        let Some(stem) = Path::new(script).file_stem() else {
            return session.source_build_script_stem();
        };
        // A returned native component borrows the UTF-8 input; this is not a
        // second source-panic classification or lossy spelling conversion.
        let Some(stem) = stem.to_str() else {
            return session.fail(Error::State);
        };
        let index = match table.merge(stem, groups.len(), session)? {
            Some(index) => index,
            None => groups.push(storage::Buffer::new(), session)?,
        };
        let Some(group) = groups.as_mut_slice().get_mut(index) else {
            return session.fail(Error::State);
        };
        group.push(script, session)?;
    }
    let mut conflicts = storage::Buffer::new();
    let mut entries = table.into_iter(session)?;
    while let Some((stem, index)) = entries.next(session)? {
        let Some(group) = groups.as_slice().get(index) else {
            return session.fail(Error::State);
        };
        if group.len() > 1 {
            conflicts.push((stem, group.as_slice()), session)?;
        }
    }
    struct Report<'a, 'b>(&'a [(&'b str, &'a [&'b str])]);
    impl std::fmt::Display for Report<'_, '_> {
        fn fmt(&self, output: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            output.write_str("found build scripts with duplicate file stems, but all build scripts must have a unique file stem")?;
            for (stem, paths) in self.0 {
                write!(output, "\n  for stem `{stem}`: ")?;
                for (index, path) in paths.iter().enumerate() {
                    if index != 0 {
                        output.write_str(", ")?;
                    }
                    output.write_str(path)?;
                }
            }
            Ok(())
        }
    }
    let message = target::formatted(format_args!("{}", Report(conflicts.as_slice())), session)?;
    if conflicts.len() == 0 {
        Ok(Ok(()))
    } else {
        Ok(Err(message))
    }
}

#[cfg(test)]
mod tests;
