// Copyright the ripgrep contributors. MIT OR Unlicense.
//! GlobSet::new's rule collection, lib.rs:466–520, before strategy compilation.
use super::{literal, matching::multi, strategy, table, Built};
use crate::input::membership::manifest::{load::formatted, session::Session, Error, Step};
use rawr_toml::document::Admission;

#[cfg(test)]
mod tests;

/// An empty input or the actual uncompiled source builders. It has no matching
/// authority; the final compiler must consume every populated strategy.
pub(super) struct Collection(Option<Rules>);

/// Reverse declaration order of GlobSet::new's local builders preserves their
/// failure/drop order while they are retained across this continuation boundary.
pub(super) struct Rules {
    pub(super) regexes: multi::Builder,
    pub(super) required: table::Regexes,
    pub(super) suffixes: multi::Builder,
    pub(super) prefixes: multi::Builder,
    pub(super) extensions: literal::Strategy,
    pub(super) basenames: literal::Strategy,
    pub(super) literals: literal::Strategy,
    pub(super) len: usize,
}

impl Collection {
    pub(super) fn new<A: Admission>(globs: &[Built], session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        // Source peeks before constructing any strategy builder.
        if globs.is_empty() {
            return Ok(Self(None));
        }
        let literals = literal::Strategy::new(literal::Field::Path);
        let basenames = literal::Strategy::new(literal::Field::Basename);
        let extensions = literal::Strategy::new(literal::Field::Extension);
        let prefixes = multi::Builder::new();
        let suffixes = multi::Builder::new();
        let required = table::Regexes::new();
        let regexes = multi::Builder::new();
        let mut rules = Rules {
            regexes,
            required,
            suffixes,
            prefixes,
            extensions,
            basenames,
            literals,
            len: 0,
        };
        for (index, glob) in globs.iter().enumerate() {
            session.items(1)?;
            rules.len = match rules.len.checked_add(1) {
                Some(length) => length,
                None => return session.fail(Error::Overflow),
            };
            match glob.parsed.strategy(session)? {
                strategy::Strategy::Literal(literal) => {
                    rules
                        .literals
                        .add(index, literal.into_string(session)?, session)?;
                }
                strategy::Strategy::Basename(literal) => {
                    rules
                        .basenames
                        .add(index, literal.into_string(session)?, session)?;
                }
                strategy::Strategy::Extension(extension) => {
                    rules
                        .extensions
                        .add(index, extension.into_string(session)?, session)?;
                }
                strategy::Strategy::Prefix(prefix) => {
                    rules
                        .prefixes
                        .add(index, prefix.into_string(session)?, session)?;
                }
                strategy::Strategy::Suffix { literal, component } => {
                    let suffix = literal.into_string(session)?;
                    session.items(1)?;
                    if component {
                        // Source keeps the full suffix live while its stripped
                        // literal is copied/inserted, then appends the suffix.
                        let Some(stripped) = suffix.get(1..) else {
                            return session.fail(Error::State);
                        };
                        let literal = formatted(format_args!("{stripped}"), session)?;
                        rules.literals.add(index, literal, session)?;
                    }
                    rules.suffixes.add(index, suffix, session)?;
                }
                strategy::Strategy::RequiredExtension(extension) => {
                    let extension = extension.into_string(session)?;
                    let regex = regex(glob, session)?;
                    rules.required.add(index, extension, regex, session)?;
                }
                strategy::Strategy::Regex => {
                    let regex = regex(glob, session)?;
                    rules.regexes.add(index, regex, session)?;
                }
            }
        }
        Ok(Self(Some(rules)))
    }

    pub(super) fn into_rules(self) -> Option<Rules> {
        self.0
    }
}

fn regex<A: Admission>(glob: &Built, session: &mut Session<'_, A>) -> Step<String> {
    let bytes = glob.regex();
    session.items(bytes.len())?;
    let Ok(text) = std::str::from_utf8(bytes) else {
        return session.fail(Error::State);
    };
    formatted(format_args!("{text}"), session)
}
