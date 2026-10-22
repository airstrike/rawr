// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.

//! ConfigValue::merge_helper's append-only key context, not a traversal stack.

use rawr_toml::document;

use super::super::super::super::{session::Session, storage, Step};

const NON_MERGEABLE_LISTS: &[&str] = &[
    "credential-alias.*",
    "doc.browser",
    "host.runner",
    "registries.*.credential-provider",
    "registry.credential-provider",
    "target.*.runner",
];

/// Borrowed bookkeeping only; this key never authorizes environment lookup.
/// The pinned merge caller appends on occupied entries and never pops.
pub(super) struct Key<'a> {
    parts: storage::Buffer<&'a str>,
}

impl<'a> Key<'a> {
    pub(super) fn new() -> Self {
        Self {
            parts: storage::Buffer::new(),
        }
    }

    pub(super) fn push<A: document::Admission>(
        &mut self,
        part: &'a str,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.string(part.len())?;
        self.parts.push(part, session)?;
        Ok(())
    }

    pub(super) fn is_nonmergeable_list<A: document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        for pattern in NON_MERGEABLE_LISTS {
            if self.matches(pattern, session)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn matches<A: document::Admission>(
        &self,
        pattern: &str,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        let mut parts = self.parts.as_slice().iter().copied();
        for expected in pattern.split('.') {
            session.items(1)?;
            // Preserve ConfigKey::matches exactly: '*' accepts a missing part,
            // too. Do not substitute a conventional same-length glob matcher.
            if parts.next() != Some(expected) && expected != "*" {
                return Ok(false);
            }
        }
        session.items(1)?;
        Ok(parts.next().is_none())
    }
}

#[cfg(test)]
mod tests;
