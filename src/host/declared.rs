//! What a facade's entries resolve against: the modules and types it
//! declares, its type aliases, and the crate types its items name.

use crate::host;

/// A facade's declarations as its items and headers resolve against them,
/// whether the facade holds them in slices or builds each from its runner
/// image on first lookup (`host::image`).
pub(crate) trait Declared {
    /// Whether the facade declares the module at `path`.
    fn module(&self, path: &host::path::Path) -> bool;

    /// The facade's type at `path`.
    fn ty(&self, path: &host::path::Path) -> Option<&host::type_def::Definition>;

    /// Whether the facade names a type alias at `path`, which holds the
    /// associated functions of the type it passes its parameters to.
    #[cfg(feature = "rust-import")]
    fn type_alias(&self, path: &host::path::Path) -> bool;

    /// How many type parameters the crate type `identity` declares, when
    /// this facade or one it resolves through declares it.
    #[cfg(feature = "rust-import")]
    fn parameters(&self, identity: &host::type_def::import::Identity) -> Option<usize>;

    /// The export of the facade that declares the crate type `identity`,
    /// which this one names and doesn't declare.
    #[cfg(feature = "rust-import")]
    fn foreign(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<&host::type_def::Export>;

    /// The path of this facade's first type of `identity`, which a message
    /// names it by.
    #[cfg(feature = "rust-import")]
    fn type_path(&self, identity: &host::type_def::import::Identity) -> Option<String>;
}

/// A facade's declarations as its builder holds them: sorted modules,
/// types in path order, its type aliases' paths and its crate types.
pub(crate) struct Slices<'facade> {
    modules: &'facade [host::path::Path],
    types: &'facade [host::type_def::Definition],
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    aliases: &'facade [host::path::Path],
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    imported: &'facade host::type_ref::ImportedTypes,
}

impl<'facade> Slices<'facade> {
    pub(crate) const fn new(
        modules: &'facade [host::path::Path],
        types: &'facade [host::type_def::Definition],
        aliases: &'facade [host::path::Path],
        imported: &'facade host::type_ref::ImportedTypes,
    ) -> Self {
        Self {
            modules,
            types,
            aliases,
            imported,
        }
    }
}

impl Declared for Slices<'_> {
    fn module(&self, path: &host::path::Path) -> bool {
        self.modules.binary_search(path).is_ok()
    }

    fn ty(&self, path: &host::path::Path) -> Option<&host::type_def::Definition> {
        self.types
            .binary_search_by(|definition| definition.path_ref().cmp(path))
            .ok()
            .map(|index| &self.types[index])
    }

    #[cfg(feature = "rust-import")]
    fn type_alias(&self, path: &host::path::Path) -> bool {
        self.aliases.contains(path)
    }

    #[cfg(feature = "rust-import")]
    fn parameters(&self, identity: &host::type_def::import::Identity) -> Option<usize> {
        self.imported.parameters(identity)
    }

    #[cfg(feature = "rust-import")]
    fn foreign(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<&host::type_def::Export> {
        self.imported.foreign(identity)
    }

    #[cfg(feature = "rust-import")]
    fn type_path(&self, identity: &host::type_def::import::Identity) -> Option<String> {
        self.types
            .iter()
            .find(|definition| definition.identity().imported() == Some(identity))
            .map(host::type_def::Definition::path)
    }
}
