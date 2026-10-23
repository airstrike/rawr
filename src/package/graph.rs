//! Canonical static source/package graph construction.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::item;
use crate::package::{error, manifest, source, Error};
use crate::{diagnostic, parse, project, Engine, ImmutableString, Position, Span};

/// Crates a built-in facade serves. A marked crate whose only Cargo
/// dependencies are these, and which has no native module, needs no
/// native build.
const FACADE_CRATES: [&str; 1] = ["iced"];

type Parts = (
    Box<[Package]>,
    Box<[source::Source]>,
    Box<[Unit]>,
    Id,
    source::Id,
);

/// An opaque package identity local to one source graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Id(usize);

/// One canonical package inside a static graph: the crate being run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    id: Id,
    name: ImmutableString,
    root: PathBuf,
    source: source::Id,
    rust_modules: Vec<RustModule>,
    cargo: Option<manifest::cargo::Cargo>,
    pub(crate) rust: Option<manifest::rust::Config>,
}

/// One native module of a marked crate: an unmarked `.rs` file in the
/// crate's module tree, compiled by Rust, never by Rawr.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustModule {
    modules: Box<[ImmutableString]>,
    path: PathBuf,
    text: String,
}

/// The file a `mod name;` declaration resolves to under Rust's rules.
enum ModuleFile {
    /// No file: the path reported missing.
    Missing(PathBuf),
    /// A `.rs` file marked `// rawr`, with the text read to see the marker.
    Marked(PathBuf, String),
    /// An unmarked `.rs` file.
    Rust(PathBuf),
}

/// A parsed source paired with its opaque graph identity.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct Unit {
    pub(crate) source: source::Id,
    pub(crate) parsed: parse::Unit,
}

/// A canonical, fully parsed static package and file-module graph.
#[derive(Debug)]
pub struct Graph {
    packages: Box<[Package]>,
    sources: Box<[source::Source]>,
    #[allow(dead_code)]
    units: Box<[Unit]>,
    entry_package: Id,
    entry_source: source::Id,
}

impl Id {
    fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) const fn index(self) -> usize {
        self.0
    }
}

impl Package {
    pub(crate) const fn id(&self) -> Id {
        self.id
    }
    /// Return the name Rawr gives the crate's items (`main`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the canonical package root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the crate root source (`src/main.rs`, `src/lib.rs` or the
    /// explicit file).
    #[must_use]
    pub const fn source(&self) -> source::Id {
        self.source
    }

    /// Return the native `.rs` modules of this package's module tree, in discovery order.
    #[must_use]
    pub fn rust_modules(&self) -> &[RustModule] {
        &self.rust_modules
    }

    /// The crate name the native modules are imported under, when the
    /// crate has native modules and its root `Cargo.toml` is a package.
    #[must_use]
    pub fn rust_crate(&self) -> Option<&str> {
        self.cargo
            .as_ref()
            .filter(|_| !self.rust_modules.is_empty())
            .map(|cargo| cargo.crate_name.as_str())
    }

    /// The root `Cargo.toml` path and exact text, when it declares a `[package]`.
    #[must_use]
    pub fn cargo_manifest(&self) -> Option<(&Path, &str)> {
        self.cargo
            .as_ref()
            .map(|cargo| (cargo.path.as_path(), cargo.text.as_str()))
    }

    #[cfg(feature = "rust-import")]
    pub(crate) const fn cargo(&self) -> Option<&manifest::cargo::Cargo> {
        self.cargo.as_ref()
    }
}

impl RustModule {
    /// Return the module path from the crate root (`["view", "widgets"]`).
    #[must_use]
    pub fn modules(&self) -> &[ImmutableString] {
        &self.modules
    }

    /// Return the canonical `.rs` file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Return the exact Rust source read while loading this graph.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

impl Graph {
    /// Load and parse one complete target graph through the supplied Engine.
    ///
    /// Every source path is canonicalized before it enters this proof
    /// bundle. Only declared `mod` edges are traversed.
    pub fn load(engine: &Engine, target: &project::Target) -> Result<Self, Error> {
        Builder::new(engine).load(target)
    }

    /// The root of the package `target` selects: the project's root, or for
    /// one file, its directory (the crate's root for a `src/` file beside a
    /// `Cargo.toml`).
    #[must_use]
    pub fn root_of(target: &project::Target) -> PathBuf {
        match target {
            project::Target::File(source) => {
                file_root(source.path(), |manifest| manifest.is_file())
            }
            project::Target::Project(project) => project.root().as_path().to_path_buf(),
        }
    }

    /// [`Graph::load`], reading each source's parse from its cell in
    /// `cells` when one holds exactly its text, and parsing the rest.
    pub fn load_with_cells(
        engine: &Engine,
        target: &project::Target,
        cells: &super::Cells,
    ) -> Result<Self, Error> {
        let mut builder = Builder::new(engine);
        builder.cells = Some(cells);
        builder.load(target)
    }

    /// Parse one runner-owned immutable snapshot through the package graph loader.
    #[doc(hidden)]
    #[cfg(feature = "rust-import")]
    pub fn load_snapshot(
        engine: &Engine,
        snapshot: &super::snapshot::Snapshot,
    ) -> Result<Self, Error> {
        Builder::from_snapshot(engine, snapshot).load_snapshot()
    }

    /// Return all packages in deterministic dependency-discovery order.
    #[must_use]
    pub fn packages(&self) -> &[Package] {
        &self.packages
    }

    /// Return all declared Rawr source snapshots.
    #[must_use]
    pub fn sources(&self) -> &[source::Source] {
        &self.sources
    }

    /// Return the application package.
    #[must_use]
    pub fn entry_package(&self) -> &Package {
        &self.packages[self.entry_package.0]
    }

    /// Return the application entry source.
    #[must_use]
    pub fn entry_source(&self) -> &source::Source {
        &self.sources[self.entry_source.index()]
    }

    /// Return whether the application package declares native Rust imports.
    #[must_use]
    pub fn declares_rust_import(&self) -> bool {
        self.entry_package().rust.is_some()
    }

    /// Every source with its parse.
    pub(crate) fn parsed_units(&self) -> impl Iterator<Item = (&source::Source, &parse::Unit)> {
        self.units
            .iter()
            .map(|unit| (&self.sources[unit.source.index()], &unit.parsed))
    }

    /// Every source with its items, as loading parsed them.
    pub(crate) fn parsed_sources(
        &self,
    ) -> impl Iterator<Item = (&source::Source, &[crate::item::Parsed])> {
        self.units
            .iter()
            .map(|unit| (&self.sources[unit.source.index()], unit.parsed.items()))
    }

    #[allow(dead_code)]
    pub(crate) fn into_parts(self) -> Parts {
        (
            self.packages,
            self.sources,
            self.units,
            self.entry_package,
            self.entry_source,
        )
    }

    /// Every `#[preview]` written in the graph's sources, without checking
    /// them, in source order.
    #[must_use]
    pub fn previews(&self) -> Vec<crate::preview::Listed> {
        self.units
            .iter()
            .flat_map(|unit| {
                let source = &self.sources[unit.source.index()];
                crate::preview::listed(source.package(), source.modules(), unit.parsed.items())
            })
            .collect()
    }

    pub(crate) fn diagnostic_sources(&self) -> Box<[diagnostic::Source]> {
        self.sources
            .iter()
            .map(|source| source.diagnostic().clone())
            .collect()
    }
}

/// The package root of a source file target: its directory, or for
/// `src/<file>.rs` of a Cargo package, the directory holding
/// `Cargo.toml`, as project discovery chooses.
fn file_root(source: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let parent = source
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    parent
        .parent()
        .filter(|root| {
            parent.file_name() == Some(std::ffi::OsStr::new("src"))
                && exists(&root.join("Cargo.toml"))
        })
        .map_or(parent.clone(), Path::to_path_buf)
}

struct Builder<'a> {
    engine: &'a Engine,
    cells: Option<&'a super::Cells>,
    #[cfg(feature = "rust-import")]
    snapshot: Option<&'a super::snapshot::Snapshot>,
    evidence: Vec<diagnostic::Source>,
    sources: Vec<source::Source>,
    units: Vec<Unit>,
    packages: Vec<Package>,
    module_targets: BTreeMap<PathBuf, error::Label>,
    active_modules: Vec<PathBuf>,
}

struct Request {
    root: PathBuf,
    source: PathBuf,
}

impl<'a> Builder<'a> {
    fn new(engine: &'a Engine) -> Self {
        Self {
            engine,
            cells: None,
            #[cfg(feature = "rust-import")]
            snapshot: None,
            evidence: Vec::new(),
            sources: Vec::new(),
            units: Vec::new(),
            packages: Vec::new(),
            module_targets: BTreeMap::new(),
            active_modules: Vec::new(),
        }
    }

    #[cfg(feature = "rust-import")]
    fn from_snapshot(engine: &'a Engine, snapshot: &'a super::snapshot::Snapshot) -> Self {
        let mut builder = Self::new(engine);
        builder.snapshot = Some(snapshot);
        builder
    }

    fn load(mut self, target: &project::Target) -> Result<Graph, Error> {
        let request = match target {
            project::Target::File(source) => Request {
                root: file_root(source.path(), |manifest| manifest.is_file()),
                source: source.path().to_path_buf(),
            },
            project::Target::Project(project) => Request {
                root: project.root().as_path().to_path_buf(),
                source: project.source().path().to_path_buf(),
            },
        };
        let entry_package = self.load_package(request)?;
        let entry_source = self.packages[entry_package.0].source;
        Ok(Graph {
            packages: self.packages.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            units: self.units.into_boxed_slice(),
            entry_package,
            entry_source,
        })
    }

    #[cfg(feature = "rust-import")]
    fn load_snapshot(mut self) -> Result<Graph, Error> {
        let snapshot = self
            .snapshot
            .expect("snapshot builder retains its immutable input");
        let entry = snapshot.entry().to_path_buf();
        let request = Request {
            root: file_root(&entry, |manifest| snapshot.read(manifest).is_some()),
            source: entry,
        };
        let entry_package = self.load_package(request)?;
        let entry_source = self.packages[entry_package.0].source;
        Ok(Graph {
            packages: self.packages.into_boxed_slice(),
            sources: self.sources.into_boxed_slice(),
            units: self.units.into_boxed_slice(),
            entry_package,
            entry_source,
        })
    }

    fn load_package(&mut self, request: Request) -> Result<Id, Error> {
        let root = self.canonical_directory(request.root)?;
        let cargo = self.load_cargo(&root)?;
        // The crate name rustc gives this target, so item paths read as
        // Rust's do. A file outside a Cargo package is `main`.
        let library = request
            .source
            .file_name()
            .is_some_and(|name| name == "lib.rs");
        let name: ImmutableString = cargo.as_ref().map_or_else(
            || "main".into(),
            |(cargo, _)| {
                if library {
                    cargo.crate_name.as_str().into()
                } else {
                    cargo.package.replace('-', "_").into()
                }
            },
        );
        let package = Id::from_index(self.packages.len());
        self.packages.push(Package {
            id: package,
            name: name.clone(),
            root: root.clone(),
            source: source::Id::from_index(0),
            rust_modules: Vec::new(),
            cargo: None,
            rust: None,
        });

        let module_root = request
            .source
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let root_source = self.load_module(
            package,
            &name,
            &root,
            &module_root,
            Box::default(),
            request.source,
            None,
            None,
        )?;
        self.packages[package.0].source = root_source;
        if let Some(cargo) = cargo {
            self.attach_cargo(package, cargo);
        }
        self.check_native_direction(package)?;
        Ok(package)
    }

    #[allow(clippy::too_many_arguments)]
    fn load_module(
        &mut self,
        package: Id,
        package_name: &str,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        path: PathBuf,
        declaration: Option<error::Label>,
        read: Option<String>,
    ) -> Result<source::Id, Error> {
        let canonical = self.canonical_source(path, package_root, declaration)?;
        if let Some(position) = self
            .active_modules
            .iter()
            .position(|path| path == &canonical)
        {
            let primary = declaration.unwrap_or_else(|| self.start_label());
            let secondary = self.module_targets.get(&canonical).copied();
            let chain = self.active_modules[position..]
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(Error::module(
                self.evidence.clone(),
                format!("file-module cycle: {chain} -> {}", canonical.display()).into(),
                primary,
                secondary,
            ));
        }
        if let Some(previous) = self.module_targets.get(&canonical).copied() {
            let primary = declaration.unwrap_or_else(|| self.start_label());
            return Err(Error::module(
                self.evidence.clone(),
                format!(
                    "more than one module declaration resolves to `{}`",
                    canonical.display()
                )
                .into(),
                primary,
                Some(previous),
            ));
        }

        let text = match read {
            Some(text) => text,
            None => self.read_to_string(&canonical).map_err(|source| {
                Error::resource(
                    self.evidence.clone(),
                    error::resource_message("read Rawr source", canonical.clone(), &source),
                    source,
                    declaration,
                )
            })?,
        };
        self.load_module_text(
            package,
            package_name,
            package_root,
            module_root,
            modules,
            canonical.clone(),
            canonical,
            text,
            declaration,
        )
    }

    /// One module's source `text`, read from `canonical` (`key` tells the
    /// module apart: an inline module shares its file's path), and the
    /// modules it declares.
    #[allow(clippy::too_many_arguments)]
    fn load_module_text(
        &mut self,
        package: Id,
        package_name: &str,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        canonical: PathBuf,
        key: PathBuf,
        text: String,
        declaration: Option<error::Label>,
    ) -> Result<source::Id, Error> {
        let id = source::Id::from_index(self.sources.len());
        let source = source::Source::new(
            id,
            canonical.clone(),
            package_name.into(),
            modules.clone(),
            text,
        )
        .map_err(Error::diagnostic)?;
        let evidence = self.push_evidence(source.diagnostic().clone());
        let parsed = match self.cells {
            Some(cells) => match cells.get(&canonical, source.text()) {
                Some(parsed) => parsed,
                None => {
                    let parsed = self
                        .engine
                        .parse_unit(source.text())
                        .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))?;
                    cells.parsed(&canonical, source.text(), &parsed);
                    parsed
                }
            },
            None => self
                .engine
                .parse_unit(source.text())
                .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))?,
        };
        let declarations = parsed
            .items()
            .iter()
            .filter_map(|item| match item {
                item::Parsed::Module(module) => Some(module.clone()),
                item::Parsed::Use(_)
                | item::Parsed::Struct(_)
                | item::Parsed::Enum(_)
                | item::Parsed::Impl(_)
                | item::Parsed::Trait(_)
                | item::Parsed::Function(_)
                | item::Parsed::TypeAlias(_)
                | item::Parsed::LegacyFunction(_) => None,
            })
            .collect::<Vec<_>>();
        let file_text = source.text().to_string();
        self.sources.push(source);
        self.units.push(Unit { source: id, parsed });
        self.active_modules.push(key.clone());
        self.module_targets.insert(
            key.clone(),
            declaration.unwrap_or(error::Label {
                source: evidence,
                span: Span::new(Position::START, Position::START),
            }),
        );

        for module in declarations {
            let mut child_modules = modules.to_vec();
            child_modules.push(module.name.text.clone());
            let label = error::Label {
                source: evidence,
                span: module.span,
            };
            // `mod name { .. }`: the text inside its braces, where it is in
            // the file, so its positions stay the file's.
            if let Some(inline) = module.inline {
                let child_key = key.join(format!("#{}", module.name.text));
                self.load_module_text(
                    package,
                    package_name,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    canonical.clone(),
                    child_key,
                    keep_span(&file_text, inline),
                    Some(label),
                )?;
                continue;
            }
            match self.module_file(module_root, &child_modules, label)? {
                ModuleFile::Missing(child_path) => {
                    self.load_module(
                        package,
                        package_name,
                        package_root,
                        module_root,
                        child_modules.into_boxed_slice(),
                        child_path,
                        Some(label),
                        None,
                    )?;
                }
                ModuleFile::Marked(child_path, text) => {
                    self.load_module(
                        package,
                        package_name,
                        package_root,
                        module_root,
                        child_modules.into_boxed_slice(),
                        child_path,
                        Some(label),
                        Some(text),
                    )?;
                }
                ModuleFile::Rust(child_path) => self.load_rust_module(
                    package,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    child_path,
                    label,
                )?,
            }
        }

        self.active_modules.pop();
        Ok(id)
    }

    /// In a marked crate, native code must not depend on marked code: a
    /// native module's `crate::…` and `super::…` paths must not land in a
    /// marked module (the crate root included). The native part is built
    /// without the marked modules, so such a path would not compile there.
    fn check_native_direction(&mut self, package: Id) -> Result<(), Error> {
        let name = self.packages[package.0].name.clone();
        let marked = self
            .sources
            .iter()
            .filter(|source| source.package() == name.as_str())
            .map(|source| (source.modules().to_vec(), source.path().to_path_buf()))
            .collect::<Vec<_>>();
        let native = self.packages[package.0]
            .rust_modules
            .iter()
            .map(|module| module.modules.to_vec())
            .collect::<Vec<_>>();
        let mut violation = None;
        'modules: for module in &self.packages[package.0].rust_modules {
            for reference in super::polyglot::references(&module.text) {
                if reference.disabled {
                    continue;
                }
                let mut base = module
                    .modules
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>();
                base.extend(reference.inline.iter().cloned());
                let supers = reference
                    .segments
                    .iter()
                    .take_while(|segment| *segment == "super")
                    .count();
                let absolute = if reference.segments[0] == "crate" {
                    reference.segments[1..].to_vec()
                } else {
                    let Some(kept) = base.len().checked_sub(supers) else {
                        continue;
                    };
                    let mut absolute = base[..kept].to_vec();
                    absolute.extend(reference.segments[supers..].iter().cloned());
                    absolute
                };
                // The innermost module the path lands in.
                let is = |modules: &[ImmutableString], length: usize| {
                    modules.len() == length
                        && modules
                            .iter()
                            .zip(&absolute)
                            .all(|(left, right)| left.as_str() == right)
                };
                for length in (0..=absolute.len()).rev() {
                    if native.iter().any(|modules| is(modules, length)) {
                        break;
                    }
                    if let Some((modules, path)) =
                        marked.iter().find(|(modules, _)| is(modules, length))
                    {
                        violation = Some((
                            module.modules.clone(),
                            module.path.clone(),
                            module.text.clone(),
                            reference,
                            modules.clone(),
                            path.clone(),
                        ));
                        break 'modules;
                    }
                }
            }
        }
        let Some((modules, path, text, reference, owner, owner_path)) = violation else {
            return Ok(());
        };
        let spelled = |modules: &[ImmutableString]| {
            std::iter::once("crate")
                .chain(modules.iter().map(ImmutableString::as_str))
                .collect::<Vec<_>>()
                .join("::")
        };
        let source = diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
            .map_err(Error::diagnostic)?;
        let index = self.push_evidence(source);
        let label = error::Label {
            source: index,
            span: manifest::byte_span(&text, reference.range.clone()),
        };
        Err(Error::module(
            self.evidence.clone(),
            format!(
                "native module `{}` uses `{used}`. \
                 `{used}` is in the marked module `{}` (`{}`). \
                 Rawr builds the native modules without the marked ones. \
                 Native code cannot use marked code. \
                 Move the item into a native module, or mark `{}` with `// rawr`.",
                spelled(&modules),
                spelled(&owner),
                owner_path.display(),
                path.display(),
                used = &text[reference.range.clone()],
            )
            .into(),
            label,
            None,
        ))
    }

    /// Rust's file rules for `mod name;`: `name.rs` or `name/mod.rs`,
    /// exactly one of them. With none, `name.rs` is reported missing. A
    /// `.rs` file is Rawr when its first line is `// rawr`.
    fn module_file(
        &self,
        module_root: &Path,
        modules: &[ImmutableString],
        declaration: error::Label,
    ) -> Result<ModuleFile, Error> {
        let mut base = module_root.to_path_buf();
        for segment in modules {
            base.push(segment.as_str());
        }
        let found = [base.with_extension("rs"), base.join("mod.rs")]
            .into_iter()
            .filter(|path| self.is_file(path))
            .collect::<Vec<_>>();
        match found.as_slice() {
            [] => Ok(ModuleFile::Missing(base.with_extension("rs"))),
            [path] => Ok(match self.read_to_string(path) {
                Ok(text) if crate::project::is_marked(&text) => {
                    ModuleFile::Marked(path.clone(), text)
                }
                _ => ModuleFile::Rust(path.clone()),
            }),
            [first, second, ..] => Err(Error::module(
                self.evidence.clone(),
                format!(
                    "file for module `{}` found at both `{}` and `{}`; delete or rename one of them",
                    modules.join("::"),
                    first.display(),
                    second.display()
                )
                .into(),
                declaration,
                None,
            )),
        }
    }

    /// Record one native module and, under Rust's rules, the modules it
    /// declares. Native code never depends on marked code: a native `mod`
    /// that resolves to a marked file is an error at load.
    fn load_rust_module(
        &mut self,
        package: Id,
        package_root: &Path,
        module_root: &Path,
        modules: Box<[ImmutableString]>,
        path: PathBuf,
        declaration: error::Label,
    ) -> Result<(), Error> {
        let canonical = self.canonical_source(path, package_root, Some(declaration))?;
        if let Some(previous) = self.module_targets.get(&canonical).copied() {
            return Err(Error::module(
                self.evidence.clone(),
                format!(
                    "more than one module declaration resolves to `{}`",
                    canonical.display()
                )
                .into(),
                declaration,
                Some(previous),
            ));
        }
        self.module_targets.insert(canonical.clone(), declaration);
        let text = self.read_to_string(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("read Rust module", canonical.clone(), &source),
                source,
                Some(declaration),
            )
        })?;
        let declarations = super::polyglot::declarations(&text);
        let mut evidence = None;
        for child in declarations {
            // `#[path]` is Rust's choice; a `#[cfg]`-disabled one is
            // never loaded by Rust.
            if child.explicit_path || child.disabled {
                continue;
            }
            let mut child_modules = modules.to_vec();
            child_modules.extend(child.path.iter().map(|segment| segment.as_str().into()));
            let index = match evidence {
                Some(index) => index,
                None => {
                    let source = diagnostic::Source::new(
                        canonical.to_string_lossy().into_owned(),
                        text.clone(),
                    )
                    .map_err(Error::diagnostic)?;
                    let index = self.push_evidence(source);
                    evidence = Some(index);
                    index
                }
            };
            let label = error::Label {
                source: index,
                span: manifest::byte_span(&text, child.range.clone()),
            };
            match self.module_file(module_root, &child_modules, label)? {
                ModuleFile::Rust(child_path) => self.load_rust_module(
                    package,
                    package_root,
                    module_root,
                    child_modules.into_boxed_slice(),
                    child_path,
                    label,
                )?,
                ModuleFile::Marked(child_path, _) => {
                    return Err(Error::module(
                        self.evidence.clone(),
                        format!(
                            "native module `crate::{}` declares `mod {};`, which is the marked file `{}`. \
                             Native Rust cannot depend on a marked module. \
                             Declare it from a marked module, or remove `// rawr` from its first line.",
                            modules.join("::"),
                            child.path.last().map_or("", String::as_str),
                            child_path.display()
                        )
                        .into(),
                        label,
                        None,
                    ));
                }
                // A missing file is Rust's own error to report when it builds.
                ModuleFile::Missing(_) => {}
            }
        }
        self.packages[package.0].rust_modules.push(RustModule {
            modules,
            path: canonical,
            text,
        });
        Ok(())
    }

    /// The crate's root `Cargo.toml`, when it is a package. Its
    /// `[dependencies]` are the Rust dependencies, and its native modules
    /// import under its crate name.
    fn load_cargo(
        &mut self,
        root: &Path,
    ) -> Result<Option<(manifest::cargo::Cargo, diagnostic::Source)>, Error> {
        let path = root.join("Cargo.toml");
        let text = match self.read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::resource(
                    self.evidence.clone(),
                    error::resource_message("read Cargo manifest", path, &source),
                    source,
                    None,
                ));
            }
        };
        let source = diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
            .map_err(Error::diagnostic)?;
        #[cfg(feature = "rust-import")]
        let ask_cargo = self.snapshot.is_none();
        #[cfg(not(feature = "rust-import"))]
        let ask_cargo = true;
        match manifest::cargo::Cargo::parse(path, text, root, ask_cargo) {
            Ok(cargo) => Ok(cargo.map(|cargo| (cargo, source))),
            Err(error) => {
                let evidence = self.push_evidence(source);
                Err(Error::manifest(
                    self.evidence.clone(),
                    evidence,
                    error.message,
                    error.span,
                ))
            }
        }
    }

    /// Attach the crate's parsed `Cargo.toml` once its module tree is read,
    /// since its native modules decide whether the crate imports Rust. Its
    /// evidence follows the module sources'.
    fn attach_cargo(
        &mut self,
        package: Id,
        (cargo, source): (manifest::cargo::Cargo, diagnostic::Source),
    ) {
        self.push_evidence(source);
        {
            let package = &mut self.packages[package.0];
            // A marked crate that uses only crates a built-in facade
            // serves runs without a native build.
            let imports = !package.rust_modules.is_empty()
                || cargo
                    .dependencies
                    .iter()
                    .any(|dependency| !FACADE_CRATES.contains(&dependency.alias.as_str()));
            if imports {
                package.rust = Some(manifest::rust::Config::from_cargo(
                    &cargo.path,
                    cargo.dependencies.clone(),
                ));
            }
        }
        self.packages[package.0].cargo = Some(cargo);
    }

    fn is_file(&self, path: &Path) -> bool {
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            return snapshot.read(path).is_some();
        }
        path.is_file()
    }

    fn canonical_directory(&self, path: PathBuf) -> Result<PathBuf, Error> {
        let primary = None;
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            let canonical = super::snapshot::normalize(path.clone()).map_err(|_| {
                self.input_error(
                    "open crate directory",
                    path.clone(),
                    io::ErrorKind::InvalidInput,
                )
            })?;
            if !snapshot.contains_directory(&canonical) {
                return Err(self.input_error(
                    "open crate directory",
                    canonical,
                    io::ErrorKind::NotFound,
                ));
            }
            return Ok(canonical);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("open crate directory", path.clone(), &source),
                source,
                primary,
            )
        })?;
        let metadata = fs::metadata(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("inspect crate directory", canonical.clone(), &source),
                source,
                primary,
            )
        })?;
        if !metadata.is_dir() {
            let source = std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "crate path is not a directory",
            );
            return Err(Error::resource(
                self.evidence.clone(),
                error::resource_message("open crate directory", canonical, &source),
                source,
                primary,
            ));
        }
        Ok(canonical)
    }

    fn canonical_source(
        &self,
        path: PathBuf,
        root: &Path,
        primary: Option<error::Label>,
    ) -> Result<PathBuf, Error> {
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            let canonical = super::snapshot::normalize(path.clone()).map_err(|_| {
                self.input_error(
                    "open declared Rawr module",
                    path.clone(),
                    io::ErrorKind::InvalidInput,
                )
            })?;
            if !canonical.starts_with(root) {
                let source = io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("source resolves outside package root `{}`", root.display()),
                );
                return Err(Error::resource(
                    self.evidence.clone(),
                    error::resource_message("open declared Rawr module", canonical, &source),
                    source,
                    primary,
                ));
            }
            if snapshot.read(&canonical).is_none() {
                return Err(self.input_error(
                    "open declared Rawr module",
                    canonical,
                    io::ErrorKind::NotFound,
                ));
            }
            return Ok(canonical);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("open declared Rawr module", path.clone(), &source),
                source,
                primary,
            )
        })?;
        if !canonical.starts_with(root) {
            let source = std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("source resolves outside package root `{}`", root.display()),
            );
            return Err(Error::resource(
                self.evidence.clone(),
                error::resource_message("open declared Rawr module", canonical, &source),
                source,
                primary,
            ));
        }
        Ok(canonical)
    }

    fn read_to_string(&self, path: &Path) -> io::Result<String> {
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            return snapshot
                .read(path)
                .map(str::to_owned)
                .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound));
        }
        fs::read_to_string(path)
    }

    #[cfg(feature = "rust-import")]
    fn input_error(&self, action: &str, path: PathBuf, kind: io::ErrorKind) -> Error {
        let source = io::Error::new(kind, "path is absent from the immutable Rawr snapshot");
        Error::resource(
            self.evidence.clone(),
            error::resource_message(action, path, &source),
            source,
            None,
        )
    }

    fn push_evidence(&mut self, source: diagnostic::Source) -> usize {
        let index = self.evidence.len();
        self.evidence.push(source);
        index
    }

    fn start_label(&self) -> error::Label {
        error::Label {
            source: 0,
            span: Span::new(Position::START, Position::START),
        }
    }
}

/// `text` with everything outside `span` blanked, line breaks kept, so the
/// text inside keeps its lines and columns.
fn keep_span(text: &str, span: Span) -> String {
    let point = |position: Position| {
        (
            position.line().unwrap_or(0),
            position.position().unwrap_or(0),
        )
    };
    let (start, end) = (point(span.start()), point(span.end()));
    let (mut line, mut column) = (1usize, 1usize);
    let mut kept = String::with_capacity(text.len());
    for character in text.chars() {
        if character == '\n' {
            kept.push('\n');
            line += 1;
            column = 1;
            continue;
        }
        let inside = start <= (line, column) && (line, column) < end;
        if inside || character == '\r' {
            kept.push(character);
        } else {
            kept.push(' ');
        }
        column += 1;
    }
    kept
}
