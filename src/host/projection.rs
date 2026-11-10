//! Sealed source-owned host projection proofs.

use crate::host;
use crate::host::runtime::type_ref::Type;
use crate::{ImmutableString, Span};
use std::collections::BTreeSet;
use std::hash::{Hash, Hasher};

/// One reusable semantic projection edge owned by its source facade.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Key {
    owner: host::facade::Dependency,
    source: host::type_def::Contract,
    projection: host::type_def::Projection,
}

/// One exact instantiated projection selected at a source location.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Site {
    key: Key,
    source: Type,
    target: Type,
    span: Span,
    #[cfg_attr(feature = "rust-import", serde(skip))]
    bound: host::runtime::Bound,
}

impl Key {
    pub(crate) fn new(
        owner: host::facade::Dependency,
        source: host::type_def::Contract,
        projection: host::type_def::Projection,
    ) -> Self {
        Self {
            owner,
            source,
            projection,
        }
    }

    pub(crate) const fn owner(&self) -> &host::facade::Dependency {
        &self.owner
    }

    pub(crate) fn diagnostic_name(&self) -> ImmutableString {
        format!("{} -> {}", self.source.path(), self.projection.target()).into()
    }
}

impl Site {
    pub(crate) const fn new(key: Key, source: Type, target: Type, span: Span) -> Self {
        Self {
            key,
            source,
            target,
            span,
            bound: host::runtime::Bound::new(),
        }
    }

    pub(crate) const fn bound(&self) -> &host::runtime::Bound {
        &self.bound
    }

    pub(crate) const fn key(&self) -> &Key {
        &self.key
    }

    pub(crate) const fn source(&self) -> &Type {
        &self.source
    }

    pub(crate) const fn target(&self) -> &Type {
        &self.target
    }

    pub(crate) const fn span(&self) -> Span {
        self.span
    }

    pub(crate) fn dependencies(&self) -> BTreeSet<host::facade::Dependency> {
        let mut dependencies = BTreeSet::from([self.key.owner.clone()]);
        dependencies.extend(self.key.projection.target_dependency());
        self.source.collect_dependencies(&mut dependencies);
        self.target.collect_dependencies(&mut dependencies);
        dependencies
    }

    pub(crate) fn symbol_id(&self) -> u64 {
        crate::program::schema::symbol_id(&(&self.key, &self.source, &self.target, self.span))
    }
}

impl PartialEq for Site {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key && self.source == other.source && self.target == other.target
    }
}

impl Eq for Site {}

impl PartialOrd for Site {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Site {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.key, &self.source, &self.target).cmp(&(&other.key, &other.source, &other.target))
    }
}

impl Hash for Site {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.source.hash(state);
        self.target.hash(state);
    }
}

impl crate::digest::Write for Key {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        writer.tag(0x2c00);
        writer.tag(0x2c01);
        self.owner.write(writer)?;
        writer.tag(0x2c02);
        self.source.write(writer)?;
        writer.tag(0x2c03);
        self.projection.write(writer)?;
        Ok(())
    }
}

impl crate::digest::Write for Site {
    fn write(&self, writer: &mut crate::digest::Writer) -> Result<(), crate::digest::Error> {
        writer.tag(0x2d00);
        writer.tag(0x2d01);
        self.key.write(writer)?;
        writer.tag(0x2d02);
        self.source.write(writer)?;
        writer.tag(0x2d03);
        self.target.write(writer)?;
        Ok(())
    }
}
