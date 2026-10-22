//! Cargo configuration conversion consumes the complete generic TOML tree.
//! It never selects just the requested key or skips unrelated invalid values.

use rawr_toml::document;

use super::super::super::{session::Session, value, walk, Diagnostic, Error, Step, Tag};
use super::definition::Definition;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Unsupported {
    Float,
    Datetime,
}

/// Complete initial file value conversion, before configuration merging.
/// All nodes retain the file's definition and borrow its admitted document.
pub(super) struct Values<'origin, 'document> {
    definition: Definition<'origin>,
    tree: value::Projection<'document>,
}

impl<'origin, 'document> Values<'origin, 'document> {
    pub(super) fn into_parts(self) -> (Definition<'origin>, value::Projection<'document>) {
        (self.definition, self.tree)
    }

    pub(super) fn from_document<A: document::Admission>(
        document: &'document document::Document<'document>,
        definition: Definition<'origin>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        // Cargo parses the whole Table before ConfigValue::from_toml runs.
        // An earlier unsupported value must not mask a later TOML conversion
        // failure such as an out-of-range integer.
        let tree = walk::toml_table(document, session)?;
        if !matches!(
            tree.nodes.as_slice().first().map(|node| node.kind),
            Some(value::Kind::Map(_))
        ) {
            return session.fail(Error::State);
        }
        // This freshly constructed Table projection is in source preorder:
        // no merge, sorted-map replay or detached nodes have been introduced.
        for node in tree.nodes.as_slice() {
            session.items(1)?;
            let unsupported = match node.kind {
                value::Kind::String(_)
                | value::Kind::I64(_)
                | value::Kind::Boolean(_)
                | value::Kind::Map(_)
                | value::Kind::Sequence(_) => continue,
                value::Kind::Float(_) => Unsupported::Float,
                value::Kind::DateTime(_) => Unsupported::Datetime,
                // Complete TOML conversion has already narrowed integers.
                _ => return session.fail(Error::State),
            };
            return session.reject(Diagnostic::at(Tag::ConfigValue(unsupported), node.span));
        }
        Ok(Self { definition, tree })
    }
}

#[cfg(test)]
mod tests;
