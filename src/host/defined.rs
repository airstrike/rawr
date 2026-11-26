//! What a native type or item is, whichever path reaches it.

use crate::{host, ImmutableString};

/// A native type or item as rustc knows it: the crate that defines it and
/// its path in that crate, whichever dependency, hidden segment or
/// re-export the program reaches it through (D1118). `iced::Background`
/// and `__rawr_source_iced_widget::core::Background` are one type, and
/// `iced::widget::container::background` and the same function through
/// `iced_widget` one function.
///
/// A crate's definition is keyed by the importer's definition identity,
/// its unit and def path. An alias has no key of its own: each path the
/// importer gives a type, an alias's among them, carries the identity of
/// the type it names. A generic instance is its definition's key with its
/// arguments, each compared by its own key where it is compared. A crate's
/// trait is keyed alike, by the trait identity its impl headers carry.
///
/// A facade's own declaration, registered by hand, has no crate
/// definition: its path in its facade keys it.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct Defined(Key);

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Key {
    /// A crate's definition, by its digest.
    Crate([u8; 32]),
    /// A registered type, by its runtime identity, path and facade.
    Registered {
        identity: host::type_def::Identity,
        path: ImmutableString,
        owner: Option<host::facade::Dependency>,
    },
    /// A registered item, by its path and facade.
    #[cfg(feature = "rust-import")]
    Declared {
        path: ImmutableString,
        owner: Option<host::facade::Dependency>,
    },
}

impl Defined {
    /// The type `definition` declares, reached through `owner`.
    pub(crate) fn of_type(
        definition: &host::type_def::Definition,
        owner: Option<&host::facade::Dependency>,
    ) -> Self {
        match definition.identity().imported() {
            Some(identity) => Self(Key::Crate(*identity.as_bytes())),
            None => Self(Key::Registered {
                identity: definition.identity().clone(),
                path: definition.path().into(),
                owner: owner.cloned(),
            }),
        }
    }

    /// The item `definition` declares, reached through `owner`: a crate's
    /// function, method or constant by its selector, which names its
    /// definition. A constructor's or field's selector names it only
    /// within its type, so its path keys it.
    #[cfg(feature = "rust-import")]
    pub(crate) fn of_item(
        definition: &host::item::Definition,
        owner: Option<&host::facade::Dependency>,
    ) -> Self {
        match definition.imported_identity().and_then(crate_item) {
            Some(digest) => Self(Key::Crate(digest)),
            None => Self(Key::Declared {
                path: definition.path().into(),
                owner: owner.cloned(),
            }),
        }
    }

    /// A digest of the key, for an id that must fit a number: equal keys
    /// give equal digests.
    pub(crate) fn digest(&self) -> u64 {
        // FNV-1a over the key's bytes.
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut write = |bytes: &[u8]| {
            for byte in bytes {
                hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3);
            }
        };
        match &self.0 {
            Key::Crate(digest) => write(digest),
            Key::Registered { path, .. } => write(path.as_bytes()),
            #[cfg(feature = "rust-import")]
            Key::Declared { path, .. } => write(path.as_bytes()),
        }
        hash
    }
}

/// The selector's digest when it names a crate's definition of its own.
#[cfg(feature = "rust-import")]
pub(crate) fn crate_item(selector: &host::item::ImportedSelector) -> Option<[u8; 32]> {
    use rawr_import::surface::identity::Selector;
    match selector {
        Selector::Function { .. }
        | Selector::Constant { .. }
        | Selector::Method { .. }
        | Selector::AssociatedFunction { .. }
        | Selector::AssociatedConstant { .. } => Some(selector.fingerprint()),
        Selector::Declaration
        | Selector::StructConstructor(_)
        | Selector::StructField(_)
        | Selector::EnumVariant { .. }
        | Selector::Unavailable => None,
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod tests {
    use crate::host::{self, facade, type_def};

    /// A crate's type the program reaches through its own dependency and
    /// through the hidden dependency a crate module's generic code goes
    /// through is one type, with one impl owner id, as rustc has it: the
    /// facades differ, and so do the paths. Another type of the crate is
    /// another (D1118).
    #[test]
    fn a_type_reached_by_two_paths_is_one_type() {
        let background = type_def::import::Identity::new([0x41; 32]);
        let color = type_def::import::Identity::new([0x42; 32]);
        let mut registry = host::Registry::new();
        for (root, module, types) in [
            (
                "iced",
                None,
                &[("Background", background), ("Color", color)][..],
            ),
            (
                "__rawr_source_iced_widget",
                Some("core"),
                &[("core::Background", background)][..],
            ),
        ] {
            let mut allocation = host::TestImportAllocation::default();
            let mut builder =
                facade::Builder::new_imported(root, facade::Abi::new(1).unwrap(), &mut allocation)
                    .unwrap();
            if let Some(module) = module {
                builder.imported_module(module, &mut allocation).unwrap();
            }
            for (path, identity) in types {
                builder
                    .imported_type(
                        path,
                        *identity,
                        &[],
                        Box::default(),
                        Box::default(),
                        type_def::Shape::Opaque.into(),
                        &mut allocation,
                    )
                    .unwrap();
            }
            registry.install(builder.finish().unwrap()).unwrap();
        }
        let defined = |path: &str| {
            super::Defined::of_type(
                registry.type_(path).unwrap(),
                Some(registry.type_owner(path).unwrap()),
            )
        };
        let public = defined("iced::Background");
        let hidden = defined("__rawr_source_iced_widget::core::Background");
        assert_ne!(
            registry.type_owner("iced::Background"),
            registry.type_owner("__rawr_source_iced_widget::core::Background")
        );
        assert_eq!(public, hidden);
        assert_eq!(
            crate::type_def::Id::foreign(&public),
            crate::type_def::Id::foreign(&hidden)
        );
        assert_ne!(public, defined("iced::Color"));
    }
}
