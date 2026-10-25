//! Canonical package- and module-qualified declaration paths.

use crate::ImmutableString;
use std::fmt;

/// The canonical path shared by stable semantic keys.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Path {
    package: ImmutableString,
    modules: Box<[ImmutableString]>,
    name: ImmutableString,
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
        }
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
