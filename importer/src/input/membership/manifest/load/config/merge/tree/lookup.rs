//! A read-only string-getter view over complete converted values.

use super::super::super::key;
use super::*;

/// Wrong final types remain data until the caller has read the environment.
pub(in super::super::super) struct Found<'a> {
    pub(in super::super::super) text: Option<&'a str>,
    pub(in super::super::super) definition: Definition<'a>,
    pub(in super::super::super) span: Option<Span>,
}

impl<'o, 'd> Tree<'o, 'd> {
    pub(in super::super::super) fn find<'a, A: document::Admission>(
        &self,
        key: &key::Key<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Found<'a>>>
    where
        'o: 'a,
        'd: 'a,
    {
        session.check()?;
        let mut index = 0;
        for part in key.parts() {
            let node = &self.nodes.as_slice()[index];
            let Some(kind) = node.kind.as_ref() else {
                return session.fail(Error::State);
            };
            let Kind::Table(table) = kind else {
                return session.reject(Diagnostic {
                    tag: Tag::ConfigLookup,
                    span: node.span,
                    expected: Some("table"),
                });
            };
            let Some(child) = table.get(part, session)? else {
                return Ok(None);
            };
            index = child;
        }
        let node = &self.nodes.as_slice()[index];
        let Some(kind) = node.kind.as_ref() else {
            return session.fail(Error::State);
        };
        session.items(1)?;
        Ok(Some(Found {
            text: match kind {
                Kind::String(text) => Some(*text),
                _ => None,
            },
            definition: node.definition,
            span: node.span,
        }))
    }
}
