//! A runner's facades as its image holds them (#1518): each type, group of
//! items, re-export and header is built the first time a lookup reaches it,
//! and kept. A runner's start reads the image's header and nothing else, so
//! what a start costs is what its program uses.

mod entry;
mod lookup;
pub(crate) mod outline;

pub(crate) use outline::Outline;

use crate::{host, Shared};
use once_cell::sync::OnceCell;
use rawr_import::surface::facade::image::Image;

pub(crate) use rawr_import::surface::facade::image::Kind;

/// A runner's image, and what has been built from it so far.
pub(crate) struct Plan {
    image: Image<'static>,
    /// Each type body's shape, converted once for every type of the body.
    shapes: Box<[OnceCell<Shared<host::type_def::Shape>>]>,
    facades: Box<[Facade]>,
}

/// One facade of an image: its root, the proof its generator digested, and
/// a slot for each entry its tables hold.
struct Facade {
    root: host::path::Path,
    /// Whether it is the facade of one of the project's path packages,
    /// which a save may change while the project runs (#1735).
    path_crate: bool,
    fingerprint: host::facade::Fingerprint,
    /// The modules the facade reaches at another path, each with the path
    /// they are held at, qualified and sorted.
    reexports: Box<[(host::path::Path, host::path::Path)]>,
    /// Its type aliases' paths, qualified, which hold the associated
    /// functions of the types they pass their parameters to.
    type_aliases: Box<[host::path::Path]>,
    types: Box<[OnceCell<Box<host::type_def::Definition>>]>,
    groups: Box<[OnceCell<Box<Group>>]>,
    aliases: Box<[OnceCell<(host::path::Path, host::path::Path)>]>,
    trait_headers: Box<[OnceCell<Option<host::facade::TraitHeader>>]>,
    impls: Box<[OnceCell<Option<host::facade::ImplHeader>>]>,
    /// Each crate type the facade's items name that another facade
    /// declares, in identity order: the identity, that facade's index, and
    /// that facade's export of it.
    foreign: Box<[Foreign]>,
    macros: OnceCell<Box<[rawr_import::surface::facade::Macro]>>,
}

struct Foreign {
    identity: host::type_def::import::Identity,
    provider: usize,
    export: OnceCell<host::type_def::Export>,
}

/// The items at one path: those a lookup finds, or a family of distinct
/// imported methods awaiting a call's evidence (`host::family`).
pub(crate) struct Group {
    items: Box<[host::item::Definition]>,
    family: Option<host::family::Pending>,
}

/// One facade of a runner's image, which the compiler's registry, its
/// facade definition and the runtime's builder share.
#[derive(Clone)]
pub(crate) struct Held {
    plan: Shared<Plan>,
    index: usize,
}

impl std::fmt::Debug for Held {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Held")
            .field("root", &self.facade().root)
            .finish_non_exhaustive()
    }
}

impl Plan {
    /// The plan `bytes` hold, a runner image of this schema, with nothing
    /// built yet: each facade's root, digest, re-exports and type alias
    /// paths, whether it is one of `path_crates`, and an empty slot for
    /// each entry.
    pub(crate) fn read(
        bytes: &'static [u8],
        path_crates: &[&str],
    ) -> Result<Shared<Self>, rawr_import::surface::Error> {
        use rawr_import::surface::Error;
        let image = Image::read(bytes)?;
        let abi = host::facade::Abi::new(1).map_err(|_| Error::Canonical)?;
        let mut facades = Vec::with_capacity(image.len());
        for index in 0..image.len() {
            let view = image.facade(index);
            let root = host::path::Path::parse(view.root()).map_err(|_| Error::Canonical)?;
            let qualify = |relative: &str| -> Result<host::path::Path, Error> {
                let relative = host::path::Path::parse(relative).map_err(|_| Error::Canonical)?;
                Ok(host::path::Path::qualify(&root, &relative))
            };
            let mut reexports = view
                .reexports()?
                .iter()
                .map(|(path, module)| Ok((qualify(path)?, qualify(module)?)))
                .collect::<Result<Vec<_>, Error>>()?;
            reexports.sort();
            let type_aliases = view
                .type_aliases()?
                .iter()
                .map(|alias| qualify(alias.path()))
                .collect::<Result<_, _>>()?;
            facades.push(Facade {
                path_crate: path_crates.contains(&view.root()),
                fingerprint: host::facade::Fingerprint::carried(&root, abi, view.digest()),
                root,
                reexports: reexports.into_boxed_slice(),
                type_aliases,
                types: slots(view.types()),
                groups: slots(view.groups()),
                aliases: slots(view.aliases()),
                trait_headers: slots(view.trait_headers()),
                impls: slots(view.impls()),
                foreign: view
                    .through()
                    .map(|(identity, provider)| Foreign {
                        identity: host::type_def::import::Identity::new(identity),
                        provider,
                        export: OnceCell::new(),
                    })
                    .collect(),
                macros: OnceCell::new(),
            });
        }
        Ok(Shared::new(Self {
            image,
            shapes: slots(image.type_bodies()),
            facades: facades.into_boxed_slice(),
        }))
    }

    /// The plan's facades, each held, in the order a runner builds them:
    /// a facade whose items name another's types after that one.
    pub(crate) fn held(plan: &Shared<Self>) -> impl Iterator<Item = Held> + '_ {
        plan.image.order().map(|index| Held {
            plan: Shared::clone(plan),
            index,
        })
    }
}

impl Held {
    fn facade(&self) -> &Facade {
        &self.plan.facades[self.index]
    }

    fn view(&self) -> rawr_import::surface::facade::image::Facade<'static> {
        self.plan.image.facade(self.index)
    }

    /// The facade's root.
    pub(crate) fn root(&self) -> &host::path::Path {
        &self.facade().root
    }

    /// Whether it is the facade of one of the project's path packages.
    pub(crate) fn is_path_crate(&self) -> bool {
        self.facade().path_crate
    }

    /// The proof of the facade's contract, which its image's generator
    /// digested.
    pub(crate) fn fingerprint(&self) -> &host::facade::Fingerprint {
        &self.facade().fingerprint
    }

    /// The modules the facade reaches at another path, each with the path
    /// they are held at, sorted.
    pub(crate) fn reexports(&self) -> &[(host::path::Path, host::path::Path)] {
        &self.facade().reexports
    }

    /// How many types, paths of items and re-exports the facade holds.
    pub(crate) fn entries(&self) -> usize {
        let view = self.view();
        view.modules() + view.types() + view.groups() + view.aliases()
    }

    /// How many of the facade's entries have been built.
    #[cfg(test)]
    pub(crate) fn built(&self) -> usize {
        let facade = self.facade();
        fn filled<T>(slots: &[OnceCell<T>]) -> usize {
            slots.iter().filter(|slot| slot.get().is_some()).count()
        }
        filled(&facade.types)
            + filled(&facade.groups)
            + filled(&facade.aliases)
            + filled(&facade.trait_headers)
            + filled(&facade.impls)
    }

    /// How many types the facade holds, at each of their paths.
    pub(crate) fn type_count(&self) -> usize {
        self.facade().types.len()
    }

    /// How many paths the facade holds items at.
    pub(crate) fn group_count(&self) -> usize {
        self.facade().groups.len()
    }

    /// The facades whose types this one's items name, as a runtime checks
    /// they are bound.
    pub(crate) fn dependencies(&self) -> std::collections::BTreeSet<host::facade::Dependency> {
        self.facade()
            .foreign
            .iter()
            .map(|foreign| {
                let provider = &self.plan.facades[foreign.provider];
                host::facade::Dependency::new(&provider.root, provider.fingerprint.clone())
            })
            .collect()
    }
}

/// `count` empty slots.
fn slots<T>(count: usize) -> Box<[OnceCell<T>]> {
    std::iter::repeat_with(OnceCell::new).take(count).collect()
}
