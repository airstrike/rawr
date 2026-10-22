//! Pinned ignore 0.4.26 rule construction for Cargo package selection.
//! Preparing a rule is not compiling a matcher or selecting physical files.
pub(super) mod compiled;
mod glob;
mod paths;
mod rule;

use super::formatted;
use crate::input::membership::manifest::{session::Session, storage::Buffer, Step};
use rawr_toml::document::Admission;
use std::path::Path;

/// Pinned GitignoreBuilder entry: own the prepared root before adding rules.
/// Compilation must follow rule collection before any matching is exposed.
pub(super) struct Builder {
    root: paths::Root,
    rules: Rules,
}
impl Builder {
    pub(super) fn build<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<compiled::Matcher, compiled::Failure>> {
        compiled::Matcher::build(self, session)
    }

    pub(super) fn prepare<A: Admission>(root: &Path, session: &mut Session<'_, A>) -> Step<Self> {
        let root = paths::Root::prepare(root, session)?;
        Ok(Self {
            root,
            rules: Rules::new(),
        })
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        line: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        self.rules.add(line, session)
    }
}

/// Ordered add_line(None, rule) state. Root preparation and matcher compilation
/// remain separate operations; this value carries no selection authority.
pub(super) struct Rules {
    patterns: Buffer<glob::Built>,
    originals: Buffer<rule::Prepared>,
}
pub(super) struct Failure {
    pub(super) original: String,
    pub(super) error: String,
}
impl Rules {
    pub(super) fn new() -> Self {
        Self {
            patterns: Buffer::new(),
            originals: Buffer::new(),
        }
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        line: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Result<(), Failure>> {
        let Some(prepared) = rule::prepare(line, session)? else {
            return Ok(Ok(()));
        };
        let built = match prepared.build(session)? {
            Ok(built) => built,
            Err(failure) => {
                // Pinned add_line maps the kind, not the outer globset error,
                // and clones the original rule before rendering that kind.
                let original = formatted(format_args!("{}", prepared.original), session)?;
                let error = formatted(format_args!("{}", failure.kind), session)?;
                return Ok(Err(Failure { original, error }));
            }
        };
        // Preserve GlobSetBuilder::add followed by the ignore metadata push.
        // A resource refusal is terminal in the shared Session, including if
        // it occurs between these two pushes.
        self.patterns.push(built, session)?;
        self.originals.push(prepared, session)?;
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests;
