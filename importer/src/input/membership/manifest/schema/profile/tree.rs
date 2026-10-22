//! Complete profile data: every recursive edge indexes one flat arena.

mod map;
mod visit;

#[cfg(test)]
pub(in crate::input::membership::manifest) mod reference;
#[cfg(test)]
mod tests;

use crate::input::membership::manifest::{
    schema::{package, primitive, trim},
    spec, storage, value, Span,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership::manifest) enum Field {
    OptLevel,
    Lto,
    CodegenBackend,
    CodegenUnits,
    Debug,
    SplitDebuginfo,
    DebugAssertions,
    Rpath,
    Panic,
    OverflowChecks,
    Incremental,
    DirName,
    Inherits,
    Strip,
    Rustflags,
    Package,
    BuildOverride,
    TrimPaths,
    HintMostlyUnused,
}

const FIELDS: [(Field, &str); 19] = [
    (Field::OptLevel, "opt-level"),
    (Field::Lto, "lto"),
    (Field::CodegenBackend, "codegen-backend"),
    (Field::CodegenUnits, "codegen-units"),
    (Field::Debug, "debug"),
    (Field::SplitDebuginfo, "split-debuginfo"),
    (Field::DebugAssertions, "debug-assertions"),
    (Field::Rpath, "rpath"),
    (Field::Panic, "panic"),
    (Field::OverflowChecks, "overflow-checks"),
    (Field::Incremental, "incremental"),
    (Field::DirName, "dir-name"),
    (Field::Inherits, "inherits"),
    (Field::Strip, "strip"),
    (Field::Rustflags, "rustflags"),
    (Field::Package, "package"),
    (Field::BuildOverride, "build-override"),
    (Field::TrimPaths, "trim-paths"),
    (Field::HintMostlyUnused, "hint-mostly-unused"),
];

/// Every converted value is present; absent fields retain the container default.
pub(in crate::input::membership::manifest) enum Value<'d> {
    Opt(super::Opt),
    Debug(super::Debug),
    Units(u32),
    Primitive(primitive::Value<'d>),
    Trim(trim::Paths),
    Package(Packages<'d>),
    BuildOverride(usize),
}

pub(in crate::input::membership::manifest) struct Entry<'d> {
    field: Field,
    position: package::Position<'d>,
    value: Value<'d>,
}

/// Original declaring location also survives typed map-value replacement.
#[derive(Clone, Copy)]
pub(in crate::input::membership::manifest) enum Location<'d> {
    Root(value::Key<'d>),
    Package {
        parent: usize,
        position: package::Position<'d>,
        key: value::Key<'d>,
    },
    BuildOverride {
        parent: usize,
        position: package::Position<'d>,
    },
}

pub(in crate::input::membership::manifest) struct Profile<'d> {
    entries: storage::Buffer<Entry<'d>>,
    ignored: storage::Buffer<value::Key<'d>>,
    span: Option<Span>,
    location: Location<'d>,
}

/// Pinned derived enum ordering puts all concrete specs before the wildcard.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::input::membership::manifest) enum PackageSpec {
    Spec(spec::Spec),
    All,
}

pub(in crate::input::membership::manifest) struct Packages<'d> {
    entries: map::Map<'d, PackageSpec>,
}

/// All source profiles are retained flat, including replaced values' callbacks.
pub(in crate::input::membership::manifest) struct Profiles<'d> {
    roots: map::Map<'d, primitive::Name<'d>>,
    nodes: storage::Buffer<Profile<'d>>,
    callbacks: storage::Buffer<(usize, usize)>,
}

impl<'d> Profiles<'d> {
    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&primitive::Name<'d>, &value::Key<'d>, &Profile<'d>)> {
        self.roots
            .entries()
            .map(|(name, key, node)| (name, key, &self.nodes.as_slice()[node]))
    }
    pub(in crate::input::membership::manifest) fn node(&self, index: usize) -> &Profile<'d> {
        &self.nodes.as_slice()[index]
    }
    pub(in crate::input::membership::manifest) fn nodes(&self) -> &[Profile<'d>] {
        self.nodes.as_slice()
    }
    pub(in crate::input::membership::manifest) fn ignored(
        &self,
    ) -> impl ExactSizeIterator<Item = (usize, &value::Key<'d>)> {
        self.callbacks
            .as_slice()
            .iter()
            .map(|&(node, key)| (node, &self.nodes.as_slice()[node].ignored.as_slice()[key]))
    }
}

impl<'d> Profile<'d> {
    pub(in crate::input::membership::manifest) fn entries(&self) -> &[Entry<'d>] {
        self.entries.as_slice()
    }
    pub(in crate::input::membership::manifest) fn get(&self, field: Field) -> Option<&Value<'d>> {
        self.entries
            .as_slice()
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| &entry.value)
    }
    pub(in crate::input::membership::manifest) fn location(&self) -> Location<'d> {
        self.location
    }
}

impl<'d> Entry<'d> {
    pub(in crate::input::membership::manifest) fn position(&self) -> package::Position<'d> {
        self.position
    }
}

impl<'d> Packages<'d> {
    pub(in crate::input::membership::manifest) fn entries(
        &self,
    ) -> impl ExactSizeIterator<Item = (&PackageSpec, &value::Key<'d>, usize)> {
        self.entries.entries()
    }
}
