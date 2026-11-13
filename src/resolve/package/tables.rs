//! The names a crate's modules bind, and what a kept resolution reads of
//! them (#1876).
//!
//! Every read goes through a method here, which notes it, with what it
//! found, for each resolution under way that is kept (`recorded`). A kept
//! resolution is reused only while each of its reads finds the same
//! (`hold`): its output is then what resolving it again gives.

use std::cell::RefCell;
use std::collections::BTreeMap;

use super::{Address, Declaration, Export, Target};
use crate::package::graph;
use crate::resolve::extern_prelude::Prelude;
use crate::{type_def, ImmutableString, Span};

pub(super) struct Tables {
    declarations: BTreeMap<Address, Declaration>,
    exports: BTreeMap<Address, Export>,
    /// Private `use` bindings, so a path may start with an imported module.
    aliases: BTreeMap<Address, Export>,
    /// A second private `use` of a name `aliases` binds, in Rust's other
    /// namespace: the function `use badge::{self, badge};` binds beside
    /// the module (`through_alias`).
    other_aliases: BTreeMap<Address, Export>,
    /// The names each module's private glob imports bind (`use super::*;`,
    /// `use Shape::*;`), which its declarations and explicit imports
    /// shadow, as in Rust.
    globs: BTreeMap<Address, Export>,
    /// Each crate enum's variant names, for `use Shape::Circle;` and
    /// `use Shape::*;`.
    enum_variants: BTreeMap<type_def::Id, Box<[ImmutableString]>>,
    packages: Box<[graph::Package]>,
    /// The crates the entry package's paths may start with
    /// (`Resolver::prelude`).
    prelude: Option<Prelude>,
    /// The reads of each kept resolution under way, innermost last.
    readers: RefCell<Vec<Reads>>,
}

/// What a kept resolution read, each with what it found.
#[derive(Clone, Default)]
pub(super) struct Reads(BTreeMap<Read, Answer>);

/// One read of the tables.
#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
enum Read {
    Declaration(Address),
    Export(Address),
    Alias(Address),
    OtherAlias(Address),
    Glob(Address),
    /// The declarations of one module of a package.
    Declared(ImmutableString, Box<[ImmutableString]>),
    /// The names one kind of import binds in one module of a package.
    Bound(Imports, ImmutableString, Box<[ImmutableString]>),
    Variants(type_def::Id),
    /// How many imports there are: a walk through them that meets a cycle
    /// ends after as many steps (`Resolver::origin`).
    Walk,
    /// Whether a path of a package is native (`Package::is_native_path`).
    Native(graph::Id, Box<[ImmutableString]>),
    /// Whether a path of a package names native code
    /// (`Package::names_native`).
    NamesNative(graph::Id, Box<[ImmutableString]>),
    Crate(graph::Id),
    Name(graph::Id),
    /// Whether the extern prelude has a crate of the name.
    Reach(ImmutableString),
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Imports {
    Exports,
    Aliases,
    Globs,
}

/// What a [`Read`] found.
#[derive(Clone, PartialEq)]
enum Answer {
    Declaration(Option<Declared>),
    Export(Option<Export>),
    Declarations(Vec<(Address, Declared)>),
    Exports(Vec<(Address, Export)>),
    Variants(Option<Box<[ImmutableString]>>),
    Count(usize),
    Holds(bool),
    Name(Option<ImmutableString>),
}

/// What a resolution that is kept reads of a declaration: whether it is
/// public, and what it declares. Where it is reaches only the errors a
/// resolution reports, and one that fails is never kept, so a line
/// written above an item keeps the resolutions that read the item.
#[derive(Clone, PartialEq)]
struct Declared {
    public: bool,
    target: Target,
}

impl Declared {
    fn of(declaration: &Declaration) -> Self {
        Self {
            public: declaration.public,
            target: declaration.target.clone(),
        }
    }

    /// Whether it is what `declaration` declares.
    fn is(&self, declaration: &Declaration) -> bool {
        self.public == declaration.public && self.target == declaration.target
    }
}

impl Tables {
    pub(super) fn new(packages: Box<[graph::Package]>, prelude: Option<Prelude>) -> Self {
        Self {
            declarations: BTreeMap::new(),
            exports: BTreeMap::new(),
            aliases: BTreeMap::new(),
            other_aliases: BTreeMap::new(),
            globs: BTreeMap::new(),
            enum_variants: BTreeMap::new(),
            packages,
            prelude,
            readers: RefCell::new(Vec::new()),
        }
    }

    /// What `read` gives, with what it read of the tables.
    pub(super) fn recorded<T>(&self, read: impl FnOnce() -> T) -> (T, Reads) {
        /// Takes the reader back off, as `read` ends or unwinds.
        struct Reader<'t>(&'t RefCell<Vec<Reads>>);

        impl Drop for Reader<'_> {
            fn drop(&mut self) {
                self.0.borrow_mut().pop();
            }
        }

        self.readers.borrow_mut().push(Reads::default());
        let reader = Reader(&self.readers);
        let value = read();
        let reads = self
            .readers
            .borrow_mut()
            .last_mut()
            .map(std::mem::take)
            .unwrap_or_default();
        drop(reader);
        (value, reads)
    }

    /// Whether each of `reads` finds what it found: the resolution that
    /// read them gives what it gave. A resolution under way that reuses it
    /// reads them too.
    pub(super) fn hold(&self, reads: &Reads) -> bool {
        if !reads
            .0
            .iter()
            .all(|(read, answer)| self.finds(read, answer))
        {
            return false;
        }
        for reader in self.readers.borrow_mut().iter_mut() {
            for (read, answer) in &reads.0 {
                if !reader.0.contains_key(read) {
                    reader.0.insert(read.clone(), answer.clone());
                }
            }
        }
        true
    }

    /// Note `read` for each resolution under way that is kept.
    fn note(&self, read: impl FnOnce() -> Read) {
        let mut readers = self.readers.borrow_mut();
        if readers.is_empty() {
            return;
        }
        let read = read();
        let mut found = None;
        for reader in readers.iter_mut() {
            if !reader.0.contains_key(&read) {
                let answer = found.get_or_insert_with(|| self.answer(&read)).clone();
                reader.0.insert(read.clone(), answer);
            }
        }
    }

    /// Whether `read` finds `answer` now, which it reads where the tables
    /// hold it.
    fn finds(&self, read: &Read, answer: &Answer) -> bool {
        let imports = |imports: &Imports| match imports {
            Imports::Exports => &self.exports,
            Imports::Aliases => &self.aliases,
            Imports::Globs => &self.globs,
        };
        match (read, answer) {
            (Read::Declaration(address), Answer::Declaration(found)) => {
                match (self.declarations.get(address), found) {
                    (Some(declaration), Some(found)) => found.is(declaration),
                    (declaration, found) => declaration.is_none() && found.is_none(),
                }
            }
            (Read::Export(address), Answer::Export(found)) => {
                self.exports.get(address) == found.as_ref()
            }
            (Read::Alias(address), Answer::Export(found)) => {
                self.aliases.get(address) == found.as_ref()
            }
            (Read::OtherAlias(address), Answer::Export(found)) => {
                self.other_aliases.get(address) == found.as_ref()
            }
            (Read::Glob(address), Answer::Export(found)) => {
                self.globs.get(address) == found.as_ref()
            }
            (Read::Declared(package, modules), Answer::Declarations(found)) => {
                let mut declared = in_module(&self.declarations, package, modules);
                found.iter().all(|(address, found)| {
                    declared
                        .next()
                        .is_some_and(|(at, declaration)| at == address && found.is(declaration))
                }) && declared.next().is_none()
            }
            (Read::Bound(kind, package, modules), Answer::Exports(found)) => {
                in_module(imports(kind), package, modules)
                    .eq(found.iter().map(|(address, export)| (address, export)))
            }
            (Read::Variants(definition), Answer::Variants(found)) => {
                self.enum_variants.get(definition) == found.as_ref()
            }
            _ => self.answer(read) == *answer,
        }
    }

    /// What `read` finds now.
    fn answer(&self, read: &Read) -> Answer {
        match read {
            Read::Declaration(address) => {
                Answer::Declaration(self.declarations.get(address).map(Declared::of))
            }
            Read::Export(address) => Answer::Export(self.exports.get(address).cloned()),
            Read::Alias(address) => Answer::Export(self.aliases.get(address).cloned()),
            Read::OtherAlias(address) => Answer::Export(self.other_aliases.get(address).cloned()),
            Read::Glob(address) => Answer::Export(self.globs.get(address).cloned()),
            Read::Declared(package, modules) => Answer::Declarations(
                in_module(&self.declarations, package, modules)
                    .map(|(address, declaration)| (address.clone(), Declared::of(declaration)))
                    .collect(),
            ),
            Read::Bound(imports, package, modules) => {
                let map = match imports {
                    Imports::Exports => &self.exports,
                    Imports::Aliases => &self.aliases,
                    Imports::Globs => &self.globs,
                };
                Answer::Exports(
                    in_module(map, package, modules)
                        .map(|(address, export)| (address.clone(), export.clone()))
                        .collect(),
                )
            }
            Read::Variants(definition) => {
                Answer::Variants(self.enum_variants.get(definition).cloned())
            }
            Read::Walk => Answer::Count(self.imports()),
            Read::Native(package, path) => {
                Answer::Holds(self.packages[package.index()].is_native_path(path))
            }
            Read::NamesNative(package, path) => {
                Answer::Holds(self.packages[package.index()].names_native(path))
            }
            Read::Crate(package) => Answer::Name(
                self.packages[package.index()]
                    .rust_crate()
                    .map(ImmutableString::from),
            ),
            Read::Name(package) => Answer::Name(Some(self.packages[package.index()].name().into())),
            Read::Reach(root) => Answer::Holds(self.reach(root)),
        }
    }

    pub(super) fn declaration(&self, address: &Address) -> Option<&Declaration> {
        self.note(|| Read::Declaration(address.clone()));
        self.declarations.get(address)
    }

    pub(super) fn export(&self, address: &Address) -> Option<&Export> {
        self.note(|| Read::Export(address.clone()));
        self.exports.get(address)
    }

    pub(super) fn alias(&self, address: &Address) -> Option<&Export> {
        self.note(|| Read::Alias(address.clone()));
        self.aliases.get(address)
    }

    pub(super) fn other_alias(&self, address: &Address) -> Option<&Export> {
        self.note(|| Read::OtherAlias(address.clone()));
        self.other_aliases.get(address)
    }

    pub(super) fn glob(&self, address: &Address) -> Option<&Export> {
        self.note(|| Read::Glob(address.clone()));
        self.globs.get(address)
    }

    /// The declarations of the module `modules` of `package`, in the order
    /// of their names.
    pub(super) fn declared_in<'t>(
        &'t self,
        package: &'t ImmutableString,
        modules: &'t [ImmutableString],
    ) -> impl Iterator<Item = (&'t Address, &'t Declaration)> + 't {
        self.note(|| Read::Declared(package.clone(), modules.into()));
        in_module(&self.declarations, package, modules)
    }

    /// The re-exports of the module `modules` of `package`.
    pub(super) fn exported_in<'t>(
        &'t self,
        package: &'t ImmutableString,
        modules: &'t [ImmutableString],
    ) -> impl Iterator<Item = (&'t Address, &'t Export)> + 't {
        self.note(|| Read::Bound(Imports::Exports, package.clone(), modules.into()));
        in_module(&self.exports, package, modules)
    }

    /// The private `use`s of the module `modules` of `package`.
    pub(super) fn aliased_in<'t>(
        &'t self,
        package: &'t ImmutableString,
        modules: &'t [ImmutableString],
    ) -> impl Iterator<Item = (&'t Address, &'t Export)> + 't {
        self.note(|| Read::Bound(Imports::Aliases, package.clone(), modules.into()));
        in_module(&self.aliases, package, modules)
    }

    /// The names the private globs of the module `modules` of `package`
    /// bind.
    pub(super) fn globbed_in<'t>(
        &'t self,
        package: &'t ImmutableString,
        modules: &'t [ImmutableString],
    ) -> impl Iterator<Item = (&'t Address, &'t Export)> + 't {
        self.note(|| Read::Bound(Imports::Globs, package.clone(), modules.into()));
        in_module(&self.globs, package, modules)
    }

    /// The variants of the crate enum `definition`, when it is one.
    pub(super) fn variants(&self, definition: &type_def::Id) -> Option<&[ImmutableString]> {
        self.note(|| Read::Variants(*definition));
        self.enum_variants.get(definition).map(AsRef::as_ref)
    }

    /// How many imports bind names, which no read notes: a walk through
    /// them that takes them all notes it ([`Self::walked_imports`]).
    pub(super) fn imports(&self) -> usize {
        self.aliases.len() + self.exports.len() + self.globs.len()
    }

    /// A walk through the imports took [`Self::imports`] steps.
    pub(super) fn walked_imports(&self) {
        self.note(|| Read::Walk);
    }

    /// The package `package`'s name.
    pub(super) fn package_name(&self, package: graph::Id) -> &str {
        self.note(|| Read::Name(package));
        self.packages[package.index()].name()
    }

    /// The crate name `package`'s native code is built as, when it has
    /// native code.
    pub(super) fn rust_crate(&self, package: graph::Id) -> Option<&str> {
        self.note(|| Read::Crate(package));
        self.packages[package.index()].rust_crate()
    }

    /// Whether `path` of `package` is native (`Package::is_native_path`).
    pub(super) fn is_native_path(&self, package: graph::Id, path: &[ImmutableString]) -> bool {
        self.note(|| Read::Native(package, path.into()));
        self.packages[package.index()].is_native_path(path)
    }

    /// Whether `path` of `package` names native code
    /// (`Package::names_native`).
    pub(super) fn names_native(&self, package: graph::Id, path: &[ImmutableString]) -> bool {
        self.note(|| Read::NamesNative(package, path.into()));
        self.packages[package.index()].names_native(path)
    }

    /// Whether the extern prelude has a crate `root`: none has when there
    /// is no prelude, whose host decides what a path reaches.
    pub(super) fn reaches(&self, root: &str) -> bool {
        self.note(|| Read::Reach(root.into()));
        self.reach(root)
    }

    fn reach(&self, root: &str) -> bool {
        self.prelude
            .as_ref()
            .is_some_and(|crates| crates.reach(root, Span::NONE).is_ok())
    }

    /// The extern prelude, which no read notes: a resolution's own.
    pub(super) const fn prelude(&self) -> Option<&Prelude> {
        self.prelude.as_ref()
    }

    /// The packages, which no read notes: what a whole program reads.
    pub(super) fn packages(&self) -> &[graph::Package] {
        &self.packages
    }

    /// Every declaration, which no read notes: what the whole program
    /// reads (`Resolver::shared_trait_names`).
    pub(super) fn declarations(&self) -> impl Iterator<Item = &Declaration> {
        self.declarations.values()
    }

    /// Every re-export, which no read notes: what the whole program reads
    /// (`Resolver::validate_exports`).
    pub(super) fn exports(&self) -> impl Iterator<Item = (&Address, &Export)> {
        self.exports.iter()
    }

    /// Whether a declaration or an import binds `address`, which no read
    /// notes: what binding glob imports reads (`Resolver::predeclare_globs`).
    pub(super) fn binds(&self, address: &Address) -> bool {
        self.declarations.contains_key(address)
            || self.exports.contains_key(address)
            || self.aliases.contains_key(address)
            || self.globs.contains_key(address)
    }

    /// Declare `declaration` at `address`; the declaration there before,
    /// when there was one.
    pub(super) fn declare(
        &mut self,
        address: Address,
        declaration: Declaration,
    ) -> Option<Declaration> {
        self.declarations.insert(address, declaration)
    }

    pub(super) fn declaration_mut(&mut self, address: &Address) -> Option<&mut Declaration> {
        self.declarations.get_mut(address)
    }

    /// Re-export `export` at `address`; the re-export there before, when
    /// there was one.
    pub(super) fn export_at(&mut self, address: Address, export: Export) -> Option<Export> {
        self.exports.insert(address, export)
    }

    /// Bind a private `use` at `address`: in the other namespace when a
    /// `use` already binds it.
    pub(super) fn alias_at(&mut self, address: Address, export: Export) {
        let aliases = if self.aliases.contains_key(&address) {
            &mut self.other_aliases
        } else {
            &mut self.aliases
        };
        aliases.insert(address, export);
    }

    /// Bind a glob import's name at `address`: a re-export when the glob is
    /// public.
    pub(super) fn glob_at(&mut self, address: Address, export: Export, public: bool) {
        let bound = if public {
            &mut self.exports
        } else {
            &mut self.globs
        };
        bound.insert(address, export);
    }

    pub(super) fn declare_variants(
        &mut self,
        definition: type_def::Id,
        variants: Box<[ImmutableString]>,
    ) {
        self.enum_variants.insert(definition, variants);
    }
}

/// The entries of `map` at the module `modules` of `package`, in the
/// order of their names: a range of the map, never a scan of it (#1850).
fn in_module<'m, V>(
    map: &'m BTreeMap<Address, V>,
    package: &'m ImmutableString,
    modules: &'m [ImmutableString],
) -> impl Iterator<Item = (&'m Address, &'m V)> + 'm {
    let from = Address {
        package: package.clone(),
        modules: modules.into(),
        name: ImmutableString::new(),
    };
    map.range(from..)
        .take_while(move |(address, _)| address.package == *package && *address.modules == *modules)
}
