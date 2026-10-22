//! Canonical static source/package graph construction.

use std::collections::BTreeMap;
#[cfg(feature = "rust-import")]
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::item;
use crate::package::{error, manifest, source, Error};
use crate::{diagnostic, parse, project, Engine, ImmutableString, Position, Span};

const CONFIG_NAME: &str = "rawr.toml";
const LIB_SOURCE: &str = "src/lib.rawr";

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

/// One manifest-declared local dependency.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dependency {
    alias: ImmutableString,
    package: Id,
}

/// One canonical package inside a static graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Package {
    id: Id,
    name: ImmutableString,
    root: PathBuf,
    manifest: Option<PathBuf>,
    manifest_text: Option<String>,
    source: source::Id,
    dependencies: Box<[Dependency]>,
    rust_modules: Vec<RustModule>,
    cargo: Option<manifest::cargo::Cargo>,
    #[cfg(feature = "rust-import")]
    pub(crate) rust: Option<manifest::rust::Config>,
}

/// One `.rs` module of a polyglot package: a module in the same tree as the
/// `.rawr` modules, compiled by Rust, never by Rawr.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RustModule {
    modules: Box<[ImmutableString]>,
    path: PathBuf,
    text: String,
}

/// The file a `mod name;` declaration resolves to under Rust's rules.
enum ModuleFile {
    Rawr(PathBuf),
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

impl Dependency {
    /// Return the lexical alias visible from the depending package.
    #[must_use]
    pub fn alias(&self) -> &str {
        &self.alias
    }

    /// Return the graph-local target package identity.
    #[must_use]
    pub const fn package(&self) -> Id {
        self.package
    }
}

impl Package {
    pub(crate) const fn id(&self) -> Id {
        self.id
    }
    /// Return the stable manifest-declared package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Return the canonical package root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Return the canonical package manifest path when one was resolved.
    #[must_use]
    pub fn manifest(&self) -> Option<&Path> {
        self.manifest.as_deref()
    }

    /// Borrow the manifest bytes consumed while resolving this exact graph.
    #[doc(hidden)]
    pub fn manifest_text(&self) -> Option<&str> {
        self.manifest_text.as_deref()
    }

    /// Return the package root source (`main.rawr` or `lib.rawr`).
    #[must_use]
    pub const fn source(&self) -> source::Id {
        self.source
    }

    /// Return declared local path dependencies in manifest order.
    #[must_use]
    pub fn dependencies(&self) -> &[Dependency] {
        &self.dependencies
    }

    /// Return the `.rs` modules of this package's module tree, in discovery order.
    #[must_use]
    pub fn rust_modules(&self) -> &[RustModule] {
        &self.rust_modules
    }

    /// The crate name the app's `.rs` modules are imported under, when
    /// the package has `.rs` modules and its root `Cargo.toml` is a package.
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
    /// Every source and manifest path is canonicalized before it enters this
    /// proof bundle. Only declared `mod` edges and local path dependencies are
    /// traversed.
    pub fn load(engine: &Engine, target: &project::Target) -> Result<Self, Error> {
        Builder::new(engine).load(target)
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
    #[cfg(feature = "rust-import")]
    #[must_use]
    pub fn declares_rust_import(&self) -> bool {
        self.entry_package().rust.is_some()
    }

    /// Every source's parsed items, as loading parsed them.
    pub(crate) fn parsed_items(&self) -> impl Iterator<Item = &crate::item::Parsed> {
        self.units.iter().flat_map(|unit| unit.parsed.items())
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
/// `src/<file>.rawr` of a Cargo package, the directory holding
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
    #[cfg(feature = "rust-import")]
    snapshot: Option<&'a super::snapshot::Snapshot>,
    evidence: Vec<diagnostic::Source>,
    sources: Vec<source::Source>,
    units: Vec<Unit>,
    packages: Vec<Package>,
    package_roots: BTreeMap<PathBuf, Id>,
    package_names: BTreeMap<ImmutableString, error::Label>,
    active_packages: Vec<PathBuf>,
    module_targets: BTreeMap<PathBuf, error::Label>,
    active_modules: Vec<PathBuf>,
}

struct Request {
    root: PathBuf,
    source: PathBuf,
    config: Option<(PathBuf, String)>,
    dependency: bool,
    origin: Option<error::Label>,
}

impl<'a> Builder<'a> {
    fn new(engine: &'a Engine) -> Self {
        Self {
            engine,
            #[cfg(feature = "rust-import")]
            snapshot: None,
            evidence: Vec::new(),
            sources: Vec::new(),
            units: Vec::new(),
            packages: Vec::new(),
            package_roots: BTreeMap::new(),
            package_names: BTreeMap::new(),
            active_packages: Vec::new(),
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
                config: None,
                dependency: false,
                origin: None,
            },
            project::Target::Project(project) => Request {
                root: project.root().as_path().to_path_buf(),
                source: project.source().path().to_path_buf(),
                config: project
                    .config()
                    .map(|config| (config.path().to_path_buf(), config.text().to_owned())),
                dependency: false,
                origin: None,
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
        let request = if entry.file_name() == Some(OsStr::new(CONFIG_NAME)) {
            let root = entry
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            let text = snapshot.read(&entry).ok_or_else(|| {
                self.input_error(
                    "read package manifest",
                    entry.clone(),
                    io::ErrorKind::NotFound,
                )
            })?;
            Request {
                root: root.clone(),
                source: root.join("src/main.rawr"),
                config: Some((entry, text.to_owned())),
                dependency: false,
                origin: None,
            }
        } else {
            Request {
                root: file_root(&entry, |manifest| snapshot.read(manifest).is_some()),
                source: entry,
                config: None,
                dependency: false,
                origin: None,
            }
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
        let root = self.canonical_directory(request.root, request.origin)?;
        if self.active_packages.contains(&root) {
            let primary = request.origin.unwrap_or_else(|| self.start_label());
            let secondary = self
                .package_roots
                .get(&root)
                .and_then(|id| self.package_names.get(&self.packages[id.0].name))
                .copied();
            return Err(Error::dependency_cycle(
                self.evidence.clone(),
                format!(
                    "local package dependency cycle reaches `{}`",
                    root.display()
                )
                .into(),
                primary,
                secondary,
            ));
        }
        if let Some(id) = self.package_roots.get(&root).copied() {
            return Ok(id);
        }

        let supplied_manifest = request.config.as_ref().map(|(path, _)| path.clone());
        let (mut manifest, manifest_source) =
            self.load_manifest(&root, request.config, request.dependency)?;
        let manifest_path = manifest
            .is_some()
            .then(|| supplied_manifest.unwrap_or_else(|| root.join(CONFIG_NAME)));
        let manifest_text = manifest_source.map(|source| self.evidence[source].text().to_owned());
        let rust = manifest.as_mut().and_then(|manifest| manifest.rust.take());
        if request.dependency && rust.is_some() {
            let source = self.manifest_source(manifest_source)?;
            let span = rust
                .as_ref()
                .map_or(Span::new(Position::START, Position::START), |config| {
                    config.span
                });
            return Err(Error::manifest(
                self.evidence.clone(),
                source,
                "only the entry Rawr package may declare `[rust]`".into(),
                span,
            ));
        }
        let name = match manifest
            .as_ref()
            .and_then(|manifest| manifest.name.as_ref())
        {
            Some(name) => name.value.clone(),
            None if request.dependency => {
                let source = self.manifest_source(manifest_source)?;
                return Err(Error::manifest(
                    self.evidence.clone(),
                    source,
                    "local dependency manifest requires `[package] name`".into(),
                    Span::new(Position::START, Position::START),
                ));
            }
            None => "main".into(),
        };
        let package = Id::from_index(self.packages.len());
        self.active_packages.push(root.clone());
        self.package_roots.insert(root.clone(), package);
        self.packages.push(Package {
            id: package,
            name: name.clone(),
            root: root.clone(),
            manifest: manifest_path,
            manifest_text,
            source: source::Id::from_index(0),
            dependencies: Box::default(),
            rust_modules: Vec::new(),
            cargo: None,
            #[cfg(feature = "rust-import")]
            rust,
        });

        let source_path = if request.dependency {
            root.join(LIB_SOURCE)
        } else {
            request.source
        };
        let module_root = source_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let root_source = self.load_module(
            package,
            &name,
            &root,
            &module_root,
            Box::default(),
            source_path,
            None,
        )?;
        self.packages[package.0].source = root_source;
        if !request.dependency {
            self.load_cargo(package, &root)?;
        }

        let name_label = match manifest
            .as_ref()
            .and_then(|manifest| manifest.name.as_ref())
        {
            Some(name) => error::Label {
                source: self.manifest_source(manifest_source)?,
                span: name.span,
            },
            None => error::Label {
                source: self.rawr_evidence(root_source)?,
                span: Span::new(Position::START, Position::START),
            },
        };
        if let Some(previous) = self.package_names.insert(name.clone(), name_label) {
            return Err(Error::duplicate_package(
                self.evidence.clone(),
                name,
                name_label,
                previous,
            ));
        }

        let mut dependencies = Vec::new();
        if let Some(manifest) = manifest {
            let source = self.manifest_source(manifest_source)?;
            for dependency in manifest.dependencies.into_vec() {
                let label = error::Label {
                    source,
                    span: dependency.span,
                };
                let dependency_root = dependency.path;
                let dependency_id = self.load_package(Request {
                    root: dependency_root.clone(),
                    source: dependency_root.join(LIB_SOURCE),
                    config: None,
                    dependency: true,
                    origin: Some(label),
                })?;
                dependencies.push(Dependency {
                    alias: dependency.alias,
                    package: dependency_id,
                });
            }
        }
        self.packages[package.0].dependencies = dependencies.into_boxed_slice();
        self.active_packages.pop();
        Ok(package)
    }

    fn load_manifest(
        &mut self,
        root: &Path,
        supplied: Option<(PathBuf, String)>,
        required: bool,
    ) -> Result<(Option<manifest::Manifest>, Option<usize>), Error> {
        let (path, text) = match supplied {
            Some(manifest) => manifest,
            None => {
                let path = root.join(CONFIG_NAME);
                match self.read_to_string(&path) {
                    Ok(text) => (path, text),
                    Err(source) if !required && source.kind() == std::io::ErrorKind::NotFound => {
                        return Ok((None, None));
                    }
                    Err(source) => {
                        return Err(Error::resource(
                            self.evidence.clone(),
                            error::resource_message("read package manifest", path, &source),
                            source,
                            None,
                        ));
                    }
                }
            }
        };
        let diagnostic = diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
            .map_err(Error::diagnostic)?;
        let source = self.push_evidence(diagnostic);
        manifest::Manifest::parse(&text, root)
            .map(|manifest| (Some(manifest), Some(source)))
            .map_err(|error| {
                Error::manifest(self.evidence.clone(), source, error.message, error.span)
            })
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

        let text = self.read_to_string(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("read Rawr source", canonical.clone(), &source),
                source,
                declaration,
            )
        })?;
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
        let parsed = self
            .engine
            .parse_unit(source.text())
            .map_err(|error| Error::parse(self.evidence.clone(), evidence, error))?;
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
                | item::Parsed::LegacyFunction(_) => None,
            })
            .collect::<Vec<_>>();
        self.sources.push(source);
        self.units.push(Unit { source: id, parsed });
        self.active_modules.push(canonical.clone());
        self.module_targets.insert(
            canonical,
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
            match self.module_file(module_root, &child_modules, label)? {
                ModuleFile::Rawr(child_path) => {
                    self.load_module(
                        package,
                        package_name,
                        package_root,
                        module_root,
                        child_modules.into_boxed_slice(),
                        child_path,
                        Some(label),
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

    /// Rust's file rules for `mod name;` across both extensions: `name.rawr`,
    /// `name.rs`, `name/mod.rawr` or `name/mod.rs`, exactly one of them. With
    /// none, the `.rawr` spelling is reported missing.
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
        let candidates = [
            (base.with_extension("rawr"), false),
            (base.with_extension("rs"), true),
            (base.join("mod.rawr"), false),
            (base.join("mod.rs"), true),
        ];
        let found = candidates
            .into_iter()
            .filter(|(path, _)| self.is_file(path))
            .collect::<Vec<_>>();
        match found.as_slice() {
            [] => Ok(ModuleFile::Rawr(base.with_extension("rawr"))),
            [(path, false)] => Ok(ModuleFile::Rawr(path.clone())),
            [(path, true)] => Ok(ModuleFile::Rust(path.clone())),
            [(first, _), (second, _), ..] => Err(Error::module(
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

    /// Record one `.rs` module and, under Rust's rules, the `.rs` modules it
    /// declares. Rust never depends on Rawr: a Rust `mod` that resolves to a
    /// `.rawr` file is an error at load.
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
                ModuleFile::Rawr(child_path) if self.is_file(&child_path) => {
                    return Err(Error::module(
                        self.evidence.clone(),
                        format!(
                            "Rust module `crate::{}` declares `mod {};`, which is the Rawr file `{}`; \
                             Rust never depends on a `.rawr` module (declare it from a `.rawr` module, \
                             or rename it to `.rs`)",
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
                ModuleFile::Rawr(_) => {}
            }
        }
        self.packages[package.0].rust_modules.push(RustModule {
            modules,
            path: canonical,
            text,
        });
        Ok(())
    }

    /// An application whose root `Cargo.toml` is a package is one Cargo
    /// crate. Its `[dependencies]` are the Rust dependencies when `rawr.toml`
    /// has no `[rust]`, and its `.rs` modules import under its crate name.
    fn load_cargo(&mut self, package: Id, root: &Path) -> Result<(), Error> {
        let path = root.join("Cargo.toml");
        let text = match self.read_to_string(&path) {
            Ok(text) => text,
            Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(Error::resource(
                    self.evidence.clone(),
                    error::resource_message("read Cargo manifest", path, &source),
                    source,
                    None,
                ));
            }
        };
        let evidence = self.push_evidence(
            diagnostic::Source::new(path.to_string_lossy().into_owned(), text.clone())
                .map_err(Error::diagnostic)?,
        );
        let cargo = manifest::cargo::Cargo::parse(path, text, root).map_err(|error| {
            Error::manifest(self.evidence.clone(), evidence, error.message, error.span)
        })?;
        let Some(cargo) = cargo else {
            return Ok(());
        };
        #[cfg(feature = "rust-import")]
        {
            let package = &mut self.packages[package.0];
            if package.rust.is_none()
                && (!cargo.dependencies.is_empty() || !package.rust_modules.is_empty())
            {
                package.rust = Some(manifest::rust::Config::from_cargo(
                    &cargo.path,
                    cargo.dependencies.clone(),
                ));
            }
        }
        self.packages[package.0].cargo = Some(cargo);
        Ok(())
    }

    fn is_file(&self, path: &Path) -> bool {
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            return snapshot.read(path).is_some();
        }
        path.is_file()
    }

    fn canonical_directory(
        &self,
        path: PathBuf,
        primary: Option<error::Label>,
    ) -> Result<PathBuf, Error> {
        #[cfg(feature = "rust-import")]
        if let Some(snapshot) = self.snapshot {
            let canonical = super::snapshot::normalize(path.clone()).map_err(|_| {
                self.input_error(
                    "open local package",
                    path.clone(),
                    io::ErrorKind::InvalidInput,
                )
            })?;
            if !snapshot.contains_directory(&canonical) {
                return Err(self.input_error(
                    "open local package",
                    canonical,
                    io::ErrorKind::NotFound,
                ));
            }
            return Ok(canonical);
        }
        let canonical = fs::canonicalize(&path).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("open local package", path.clone(), &source),
                source,
                primary,
            )
        })?;
        let metadata = fs::metadata(&canonical).map_err(|source| {
            Error::resource(
                self.evidence.clone(),
                error::resource_message("inspect local package", canonical.clone(), &source),
                source,
                primary,
            )
        })?;
        if !metadata.is_dir() {
            let source = std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "local package path is not a directory",
            );
            return Err(Error::resource(
                self.evidence.clone(),
                error::resource_message("open local package", canonical, &source),
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

    fn rawr_evidence(&self, id: source::Id) -> Result<usize, Error> {
        let diagnostic = self.sources[id.index()].diagnostic();
        self.evidence
            .iter()
            .position(|source| source == diagnostic)
            .ok_or_else(|| {
                Error::authority(
                    self.evidence.clone(),
                    "parsed source lacks matching diagnostic evidence".into(),
                )
            })
    }

    fn manifest_source(&self, source: Option<usize>) -> Result<usize, Error> {
        source.ok_or_else(|| {
            Error::authority(
                self.evidence.clone(),
                "parsed manifest lacks source identity evidence".into(),
            )
        })
    }

    fn start_label(&self) -> error::Label {
        error::Label {
            source: 0,
            span: Span::new(Position::START, Position::START),
        }
    }
}
