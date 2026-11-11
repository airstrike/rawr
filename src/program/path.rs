//! Canonical package- and module-qualified declaration paths.

use crate::ImmutableString;
use std::fmt;

/// The canonical path shared by stable semantic keys.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "rust-import", derive(serde::Deserialize, serde::Serialize))]
pub(crate) struct Path {
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
    name: ImmutableString,
    /// For an item declared in a block, its place among the module's block
    /// items of its name, from 1: two bodies' `struct Point` are two items.
    /// A path never prints it, as rustc's `type_name` names both
    /// `main::Point`.
    #[cfg_attr(feature = "rust-import", serde(default))]
    block: Option<u32>,
}

impl fmt::Display for Path {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.package)?;

        for module in &self.modules {
            write!(formatter, "::{module}")?;
        }

        write!(formatter, "::{}", self.name)
    }
}

impl Path {
    pub(crate) fn new(
        package: ImmutableString,
        modules: Box<[ImmutableString]>,
        name: ImmutableString,
    ) -> Self {
        Self {
            package,
            modules,
            name,
            block: None,
        }
    }

    /// This path for the `block`th block item of its name in its module.
    pub(crate) fn in_block(self, block: Option<u32>) -> Self {
        Self { block, ..self }
    }

    /// The path's place among its module's block items of its name.
    pub(crate) const fn block(&self) -> Option<u32> {
        self.block
    }

    pub(crate) fn package(&self) -> &str {
        &self.package
    }

    pub(crate) fn modules(&self) -> &[ImmutableString] {
        &self.modules
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }
}
