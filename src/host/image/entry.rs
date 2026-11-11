//! Building a held facade's entries from its image, each the first time a
//! lookup reaches it, as a runner's facade builder would have built it.

use super::{Group, Held};
use crate::package::import::runner;
use crate::{host, ImmutableString};

/// The account a held facade's entries are built under: none is counted
/// (D674), and an amount that can't be represented is refused.
struct Unbounded;

impl host::ImportAllocation for Unbounded {
    fn retain_checked(
        &mut self,
        items: Option<usize>,
        bytes: Option<usize>,
    ) -> Result<(), rawr_import::surface::Error> {
        match (items, bytes) {
            (Some(_), Some(_)) => Ok(()),
            _ => Err(rawr_import::surface::Error::Allocation),
        }
    }
}

/// `result`'s entry, which its runner's image holds. The image is rawr's
/// own, so an entry rawr can't build is a rawr bug.
fn built<T>(root: &host::path::Path, path: &str, result: Result<T, runner::Error>) -> T {
    result.unwrap_or_else(|error| {
        panic!(
            "internal error: rawr cannot build `{root}::{path}` from its runner's bindings: \
             {error}. This is a rawr bug"
        )
    })
}

impl Group {
    /// The items a lookup of the group's path finds, in fingerprint order.
    pub(crate) fn items(&self) -> &[host::item::Definition] {
        &self.items
    }

    /// The distinct imported methods at the path, when they await a call's
    /// evidence.
    pub(crate) fn family(&self) -> Option<&host::family::Pending> {
        self.family.as_ref()
    }
}

impl Held {
    /// `relative`, an image's path below the root, qualified.
    pub(super) fn qualified(&self, relative: &str) -> Result<host::path::Path, host::Error> {
        Ok(host::path::Path::qualify(
            self.root(),
            &host::path::Path::parse(relative)?,
        ))
    }

    /// The type at `index`.
    pub(crate) fn type_at(&self, index: usize) -> &host::type_def::Definition {
        self.facade().types[index].get_or_init(|| {
            let path = self.view().type_path(index);
            Box::new(built(self.root(), path, self.build_type(index)))
        })
    }

    fn build_type(&self, index: usize) -> Result<host::type_def::Definition, runner::Error> {
        let view = self.view();
        let ty = view.type_definition(index)?;
        let shape = self.plan.shapes[view.type_body(index)]
            .get_or_try_init(|| runner::shape(ty.shape(), ty.layout()).map(crate::Shared::new))?
            .clone();
        runner::imported_type(self.qualified(view.type_path(index))?, &ty, shape)
    }

    /// The items and family at the path of index `index`.
    pub(crate) fn group_at(&self, index: usize) -> &Group {
        self.facade().groups[index].get_or_init(|| {
            let path = self.view().group(index).path();
            Box::new(built(self.root(), path, self.build_group(index)))
        })
    }

    /// The group as a runner's facade builder makes it: each item's forms
    /// admitted in the facade's order, then its macros, each resolved, and
    /// the whole partitioned into items or a family (`host::family`).
    fn build_group(&self, index: usize) -> Result<Group, runner::Error> {
        let group = self.view().group(index);
        let root = self.root();
        let path = self.qualified(group.path())?;
        let mut declarations = Vec::new();
        for item in group.items()? {
            // The item, and its form that a value fills, when it has one.
            for form in std::iter::once(item.signature()).chain(item.values()) {
                let signature = runner::signature(root.name(), form)?;
                let definition = runner::item_definition(&item, form, signature)?;
                host::facade::admit_imported(&mut declarations, definition, &path)?;
            }
        }
        if group.macros().len() != 0 {
            let macros = self.macros()?;
            for rules in group.macros().map(|index| &macros[index]) {
                let definition = host::item::Definition::macro_rules(
                    rules.path(),
                    rules.source(),
                    format!("{root}::{}", rules.krate()),
                )?;
                match host::facade::admit(&mut declarations, definition, &path) {
                    Ok(()) | Err(host::Error::DuplicateFacadeEntry { .. }) => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        let mut resolved = Vec::with_capacity(declarations.len());
        for declaration in declarations {
            resolved.push(declaration.resolve_imported(root, self, &mut Unbounded)?);
        }
        resolved.sort_by_key(host::item::Definition::fingerprint);
        Ok(
            match host::family::Pending::partition(resolved, &mut Unbounded)? {
                host::family::Admission::Ordinary(mut items) => {
                    host::facade::sort_items(&mut items)?;
                    Group {
                        items: items.into_boxed_slice(),
                        family: None,
                    }
                }
                host::family::Admission::Pending(family) => Group {
                    items: Box::default(),
                    family: Some(family),
                },
            },
        )
    }

    fn macros(&self) -> Result<&[rawr_import::surface::facade::Macro], runner::Error> {
        Ok(self
            .facade()
            .macros
            .get_or_try_init(|| self.view().macros().map(Vec::into_boxed_slice))?)
    }

    /// The re-export at `index`: its path and the path it names, as the
    /// facade's builder qualifies them (a builtin's and a std item's are
    /// their own).
    pub(crate) fn alias_at(&self, index: usize) -> &(host::path::Path, host::path::Path) {
        self.facade().aliases[index].get_or_init(|| {
            let alias = self.view().alias(index);
            let pair = (|| -> Result<_, runner::Error> {
                let target = match alias.kind {
                    rawr_import::surface::facade::image::Kind::Variant => {
                        self.qualified(alias.target)?
                    }
                    rawr_import::surface::facade::image::Kind::Builtin
                    | rawr_import::surface::facade::image::Kind::Std => {
                        host::path::Path::parse(alias.target)?
                    }
                };
                Ok((self.qualified(alias.path)?, target))
            })();
            built(self.root(), alias.path, pair)
        })
    }

    /// The trait header at `index`, when it resolves.
    pub(crate) fn trait_header_at(&self, index: usize) -> Option<&host::facade::TraitHeader> {
        self.facade().trait_headers[index]
            .get_or_init(|| {
                let view = self.view();
                let path = view.trait_header_path(index);
                let header = built(
                    self.root(),
                    path,
                    view.trait_header(index).map_err(Into::into),
                );
                let defaults = header
                    .defaults()
                    .iter()
                    .map(|default| default.as_ref().map(runner::ty).transpose())
                    .collect::<Result<Box<[_]>, _>>()
                    .ok()?;
                host::facade::TraitHeader {
                    path: built(self.root(), path, self.qualified(path).map_err(Into::into)),
                    identity: *header.identity(),
                    parameters: header.parameters().map(ImmutableString::from).collect(),
                    defaults,
                }
                .resolve(self.root(), self)
            })
            .as_ref()
    }

    /// The impl header at `index`, when it resolves.
    pub(crate) fn impl_at(&self, index: usize) -> Option<&host::facade::ImplHeader> {
        self.facade().impls[index]
            .get_or_init(|| {
                let header = built(
                    self.root(),
                    "<impl>",
                    self.view().impl_header(index).map_err(Into::into),
                );
                runner::impl_header(&header)?.resolve(self.root(), self)
            })
            .as_ref()
    }

    /// The facade's type aliases, resolved, as its registry holds them.
    pub(crate) fn type_aliases(&self) -> Result<Vec<host::facade::TypeAlias>, runner::Error> {
        let mut resolved = Vec::new();
        for alias in self.view().type_aliases()? {
            let defaults = alias
                .defaults()
                .iter()
                .map(|default| default.as_ref().map(runner::ty).transpose())
                .collect::<Result<Vec<_>, _>>()?
                .into_boxed_slice();
            resolved.push(host::facade::TypeAlias::imported(
                self.root(),
                self.qualified(alias.path())?,
                alias.parameters().map(ImmutableString::from).collect(),
                defaults,
                runner::ty(alias.target())?,
                self,
            )?);
        }
        resolved.sort_by(|left, right| left.path().cmp(right.path()));
        Ok(resolved)
    }

    /// The export of the facade's first type of `identity`, in path order,
    /// as another facade's items name it.
    pub(crate) fn export(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<host::type_def::Export> {
        let first = self.identity_types(identity).into_iter().min()?;
        Some(host::type_def::Export::new(
            self.root().clone(),
            self.fingerprint().clone(),
            self.type_at(first).contract(),
        ))
    }
}

impl host::declared::Declared for Held {
    fn module(&self, path: &host::path::Path) -> bool {
        Held::module(self, path)
    }

    fn ty(&self, path: &host::path::Path) -> Option<&host::type_def::Definition> {
        self.type_index(path).map(|index| self.type_at(index))
    }

    fn type_alias(&self, path: &host::path::Path) -> bool {
        self.facade().type_aliases.contains(path)
    }

    fn parameters(&self, identity: &host::type_def::import::Identity) -> Option<usize> {
        self.view()
            .identity(identity.as_bytes())
            .map(|types| types.parameters())
    }

    fn foreign(
        &self,
        identity: &host::type_def::import::Identity,
    ) -> Option<&host::type_def::Export> {
        let foreign = self.facade().foreign.as_ref();
        let index = foreign
            .binary_search_by(|candidate| candidate.identity.cmp(identity))
            .ok()?;
        let entry = &foreign[index];
        Some(entry.export.get_or_init(|| {
            Held {
                plan: crate::Shared::clone(&self.plan),
                index: entry.provider,
            }
            .export(identity)
            .expect("a facade's image names the facade that declares each type it resolves through")
        }))
    }

    fn type_path(&self, identity: &host::type_def::import::Identity) -> Option<String> {
        let first = self.identity_types(identity).into_iter().min()?;
        Some(format!("{}::{}", self.root(), self.view().type_path(first)))
    }
}
