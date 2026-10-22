//! Stable nominal identities and generation-local type definitions.

use crate::{capability, item, program, ImmutableString, Span};
use std::fmt;
#[cfg(feature = "no_std")]
use std::prelude::v1::*;

/// A stable package- and module-qualified script type key.
///
/// Keys survive recompilation while the declaration's package, module path,
/// and declared name remain unchanged.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Key(program::Path);

impl fmt::Display for Key {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Key {
    pub(crate) fn new(
        package: ImmutableString,
        modules: Box<[ImmutableString]>,
        name: ImmutableString,
    ) -> Self {
        Self(program::Path::new(package, modules, name))
    }

    /// Return the canonical package component.
    #[must_use]
    pub fn package(&self) -> &str {
        self.0.package()
    }

    /// Return the canonical module-path components.
    #[must_use]
    pub fn modules(&self) -> &[ImmutableString] {
        self.0.modules()
    }

    /// Return the declared type name.
    #[must_use]
    pub fn name(&self) -> &str {
        self.0.name()
    }
}

/// Stable semantic identity for a host or script type.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Identity {
    /// A linked or symbolic imported type registered by the host.
    Host(crate::host::type_def::Identity),
    /// A package- and module-qualified script type.
    Script(Key),
}

/// A generation-local resolved type identifier.
///
/// IDs can only be allocated while resolving a complete compilation unit.
/// Stable runtime identity uses [`Identity`] instead.
///
/// ```compile_fail
/// let _ = rawr::type_def::Id(0);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

impl Id {
    pub(crate) const fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }

    /// A builtin type as a trait impl owner (`impl Shape for i64`),
    /// by its witness code (`-2` for `i64`): an id no script type has.
    pub(crate) const fn builtin(code: i64) -> Self {
        Self(usize::MAX - code.unsigned_abs() as usize)
    }

    /// A facade type as a trait impl owner (`impl From<Shade> for
    /// iced::Color`), by its path: an id no script or builtin type has.
    pub(crate) fn foreign(path: &str) -> Self {
        // FNV-1a, folded below the builtin codes' 64 ids.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in path.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
        Self(usize::MAX - 64 - (hash % (1 << 40)) as usize)
    }

    /// The witness code of a builtin impl owner's id.
    pub(crate) const fn builtin_code(self) -> Option<i64> {
        let distance = usize::MAX - self.0;
        if distance < 64 {
            Some(-(distance as i64))
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) enum Kind {
    Struct,
    Enum,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Declaration {
    id: Id,
    item: item::Id,
    key: Key,
    name: ImmutableString,
    name_span: Span,
    kind: Kind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Definition {
    id: Id,
    item: item::Id,
    key: Key,
    name: ImmutableString,
    name_span: Span,
    kind: Kind,
    schema: program::schema::Fingerprint,
    capabilities: capability::Set,
}

impl Declaration {
    pub(crate) fn new(
        id: Id,
        item: item::Id,
        key: Key,
        name: ImmutableString,
        name_span: Span,
        kind: Kind,
    ) -> Self {
        Self {
            id,
            item,
            key,
            name,
            name_span,
            kind,
        }
    }

    pub(crate) const fn id(&self) -> Id {
        self.id
    }

    pub(crate) fn resolve(
        self,
        schema: program::schema::Fingerprint,
        capabilities: capability::Set,
    ) -> Definition {
        Definition {
            id: self.id,
            item: self.item,
            key: self.key,
            name: self.name,
            name_span: self.name_span,
            kind: self.kind,
            schema,
            capabilities,
        }
    }
}

impl Definition {
    /// The owner of trait impls for the builtin type `name`, which
    /// has no declaration: keyed `std::name`, with the witness `code`.
    pub(crate) fn builtin(code: i64, name: &str, item: item::Id, name_span: Span) -> Self {
        Self::outside(Id::builtin(code), name, item, name_span)
    }

    /// The owner of trait impls for the facade type at `path`, which has
    /// no declaration in the crate: keyed `std::path`.
    pub(crate) fn foreign(path: &str, item: item::Id, name_span: Span) -> Self {
        Self::outside(Id::foreign(path), path, item, name_span)
    }

    fn outside(id: Id, name: &str, item: item::Id, name_span: Span) -> Self {
        let key = Key::new("std".into(), Box::default(), name.into());
        let schema = program::schema::for_builtin(&key);
        Declaration::new(id, item, key, name.into(), name_span, Kind::Struct)
            .resolve(schema, capability::Set::new())
    }
}

#[allow(dead_code)]
impl Definition {
    pub(crate) const fn id(&self) -> Id {
        self.id
    }

    pub(crate) const fn item(&self) -> item::Id {
        self.item
    }

    pub(crate) fn key(&self) -> &Key {
        &self.key
    }

    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    pub(crate) const fn name_span(&self) -> Span {
        self.name_span
    }

    pub(crate) const fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn schema(&self) -> &program::schema::Fingerprint {
        &self.schema
    }

    pub(crate) const fn capabilities(&self) -> capability::Set {
        self.capabilities
    }
}
