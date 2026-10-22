// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.

//! Path-base lookup key correspondence: Cargo context/key.rs and toml/mod.rs.

use std::alloc::Layout;

use rawr_toml::document;

#[cfg(test)]
use super::super::super::storage;
use super::super::super::{scalar, session::Session, Error, Step};
use super::super::features::{Feature, Features};
use crate::input::membership::host;

/// The original TOML component and its exact environment key/prefix share one
/// feature-checked derivation. This is a lookup request, not a resolved base.
pub(super) struct Key<'name> {
    parts: [&'name str; 3],
    length: usize,
    prefix: String,
}

impl<'name> Key<'name> {
    pub(super) fn path_base<A: document::Admission>(
        base: scalar::Name<'name>,
        features: &Features<'_, '_>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        if base.kind() != scalar::Kind::PathBase {
            return session.fail(Error::State);
        }
        // Cargo requires the feature before constructing or looking up the key.
        features.require(Feature::PathBases, session)?;
        Self::encode(base.as_str(), session)
    }

    pub(super) fn declared_path_base<'base, A: document::Admission>(
        base: &'base super::super::super::schema::primitive::Name<'_>,
        features: &Features<'_, '_>,
        session: &mut Session<'_, A>,
    ) -> Step<Key<'base>> {
        session.check()?;
        if base.kind() != scalar::Kind::PathBase {
            return session.fail(Error::State);
        }
        features.require(Feature::PathBases, session)?;
        Key::encode(base.as_str(), session)
    }

    fn encode<A: document::Admission>(
        base: &'name str,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Self::encode_parts(
            ["path-bases", base, ""],
            2,
            base,
            "CARGO_PATH_BASES_",
            "_",
            session,
        )
    }

    pub(super) fn registry<A: document::Admission>(
        registry: scalar::Name<'name>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        if registry.kind() != scalar::Kind::Registry {
            return session.fail(Error::State);
        }
        let name = registry.as_str();
        Self::encode_parts(
            ["registries", name, "index"],
            3,
            name,
            "CARGO_REGISTRIES_",
            "_INDEX_",
            session,
        )
    }

    pub(super) fn registry_index<A: document::Admission>(
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        Self::encode_parts(
            ["registry", "index", ""],
            2,
            "",
            "CARGO_REGISTRY_INDEX",
            "_",
            session,
        )
    }

    fn encode_parts<A: document::Admission>(
        parts: [&'name str; 3],
        count: usize,
        base: &str,
        start: &str,
        end: &str,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.string(base.len())?;
        let mut length = start.len();
        for ch in mapped(base) {
            length = match length.checked_add(ch.len_utf8()) {
                Some(length) => length,
                None => return session.fail(Error::Overflow),
            };
        }
        length = match length.checked_add(end.len()) {
            Some(length) => length,
            None => return session.fail(Error::Overflow),
        };
        session.string(length)?;
        session.items(1)?;
        let layout = match Layout::array::<u8>(length) {
            Ok(layout) => layout,
            Err(_) => return session.fail(Error::Overflow),
        };
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut prefix = String::new();
        if let Err(error) = prefix.try_reserve_exact(length) {
            return session.fail(Error::Allocation(error));
        }
        #[cfg(test)]
        storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
        prefix.push_str(start);
        prefix.extend(mapped(base));
        prefix.push_str(end);
        Ok(Self {
            parts,
            length: count,
            prefix,
        })
    }

    pub(super) fn parts(&self) -> &[&str] {
        &self.parts[..self.length]
    }

    pub(super) fn environment(&self) -> &str {
        &self.prefix[..self.prefix.len() - 1]
    }

    pub(super) fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Only the environment arm of Cargo's has_key. False still requires the
    /// effective configuration lookup; true does not bypass its type checking.
    pub(super) fn has_environment(
        &self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<bool> {
        if session
            .environment(self.environment().as_bytes())?
            .is_some()
        {
            return Ok(true);
        }
        session.environment_prefix(self.prefix().as_bytes())
    }
}

fn mapped(base: &str) -> impl Iterator<Item = char> + '_ {
    base.chars()
        .map(|ch| if ch == '-' { '_' } else { ch })
        .flat_map(char::to_uppercase)
}

#[cfg(test)]
mod tests;
