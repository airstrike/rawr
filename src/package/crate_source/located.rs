//! Where a crate module a program names is defined: the program's path to
//! it read through the crates' modules and re-exports, as rustc reads it.
//! `iced::widget::container` is `iced_widget`'s `container` through iced's
//! `pub mod widget { pub use iced_widget::*; }`, and `iced::border` is
//! `iced_core`'s `border` through iced's `pub use crate::core::border` and
//! the `core` each crate on the way names by a `use`. Cargo says where each
//! package of the program's graph is, what each package names its
//! dependencies, and which features each has.
//!
//! The program reaches the defining crate's root, which the module's
//! `crate::` names, by a dependency of its own on that crate under a hidden
//! name ([`Located::dependency`]), as the runner reaches each crate of its
//! closure. A crate may hide its root from the program's paths, as iced's
//! `widget` hides `iced_widget`'s `core` by a private `mod core {}`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use crate::package::manifest::rust::{Dependency, GitReference, Source};
use crate::package::polyglot::{self, Token};
use crate::project::cargo::features::Enabled;
use crate::ImmutableString;

/// How many re-exports a path may go through: a guard against a cycle of
/// globs, which rustc rejects.
const HOPS: usize = 32;

/// The prefix of the name of the program's dependency on a crate-source
/// module's defining crate ([`Located::alias`]).
const HIDDEN: &str = "__rawr_source_";

/// A crate module the program names, found.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    /// The module's file.
    pub(crate) file: PathBuf,
    /// The package that defines it.
    pub(crate) package: Package,
    /// The module's path from its crate's root.
    pub(crate) module: Box<[ImmutableString]>,
    /// The features Cargo resolved for the defining package, which its
    /// `cfg`s read.
    pub(crate) features: Vec<String>,
    /// The packages of the program's graph from the defining package's
    /// source, itself among them.
    pub(crate) siblings: Vec<Package>,
    /// The program's path to the module whose items its own are, when its
    /// parent declares it privately and re-exports it by a public glob:
    /// `iced::widget` for `iced::widget::helpers`, which `iced_widget`
    /// declares as `mod helpers; pub use helpers::*;`. The program names
    /// its items only there.
    pub(crate) mount: Option<Box<[ImmutableString]>>,
    /// Whether the program names the module's crate by a dependency of its
    /// own. One it reaches only through others, as `iced_core` behind
    /// `iced`, it names by its hidden dependency on it ([`Located::alias`]).
    pub(crate) named: bool,
    /// Whether the defining crate is a dependency of the program's own.
    direct: bool,
}

/// A package of the program's graph, as `cargo metadata` describes it.
#[derive(Clone, Debug)]
pub(crate) struct Package {
    id: String,
    /// Its name.
    pub(crate) name: String,
    /// Its manifest's directory.
    pub(crate) directory: PathBuf,
    /// Its source, `registry+..` or `git+..`, or none for a path package.
    pub(crate) source: Option<String>,
    version: String,
    /// Its library, which a binary-only package lacks.
    library: Option<Library>,
}

/// A package's library target.
#[derive(Clone, Debug)]
struct Library {
    /// Its crate name, as code writes it.
    name: String,
    /// Its root module's file.
    root: PathBuf,
}

/// The program's package graph.
struct Graph {
    packages: Vec<Package>,
    nodes: Vec<Node>,
    /// The program's own package id.
    root: String,
    /// Each file read so far, as its package's features select its items.
    texts: RefCell<BTreeMap<PathBuf, Rc<str>>>,
    /// Each name looked up in a module so far: what it names, `None` while
    /// the lookup is under way, so a cycle of globs ends.
    children: RefCell<BTreeMap<Lookup, Option<Module>>>,
}

/// One package of Cargo's resolved graph.
struct Node {
    id: String,
    /// Its dependencies, each by the name its code writes it by.
    dependencies: Vec<Edge>,
    /// Its features, as Cargo resolved them.
    features: Vec<String>,
}

/// A dependency of a [`Node`].
struct Edge {
    /// The name the dependent's code writes it by.
    name: String,
    /// Its package id.
    package: String,
}

/// A name looked up in a module ([`Graph::child`]).
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Lookup {
    package: usize,
    path: Vec<ImmutableString>,
    name: String,
}

/// A module of a package of the graph.
#[derive(Clone)]
struct Module {
    /// The package, by its index in [`Graph::packages`].
    package: usize,
    /// Its path from the package's root module.
    path: Vec<ImmutableString>,
    /// The file it is in, which an inline module shares with its parent.
    file: PathBuf,
    /// Its text: its file's, or the inside of its braces.
    text: Rc<str>,
    /// The directory its `mod name;` children's files are in.
    directory: PathBuf,
}

/// A name a `use` declaration binds, or its glob.
enum Bound {
    Name(String),
    Glob,
}

/// One import a `use` declaration makes: the path it imports and what it
/// binds that to.
struct Import {
    path: Vec<String>,
    bound: Bound,
}

/// The crate module at `path`, the program's path to it from a dependency's
/// name, in the program whose manifest is `manifest`. `None` when Cargo
/// can't describe the graph offline or the path names no module.
pub(crate) fn locate(manifest: &Path, path: &[ImmutableString]) -> Option<Located> {
    let graph = Graph::read(manifest)?;
    let named = graph
        .index(&graph.root)
        .zip(path.first())
        .is_some_and(|(root, first)| graph.dependency(root, first).is_some());
    let module = graph.module(path)?;
    if module.path.is_empty() {
        return None;
    }
    let direct = graph.index(&graph.root).is_some_and(|root| {
        graph.node(root).is_some_and(|node| {
            node.dependencies
                .iter()
                .any(|edge| graph.index(&edge.package) == Some(module.package))
        })
    });
    let package = graph.packages[module.package].clone();
    let siblings: Vec<Package> = graph
        .packages
        .iter()
        .filter(|other| other.source.is_some() && other.source == package.source)
        .cloned()
        .collect();
    let mount = graph
        .globbed_privately(&module)
        .then(|| path[..path.len() - 1].into());
    Some(Located {
        features: graph.features(module.package).to_vec(),
        mount,
        named,
        direct,
        file: module.file,
        package,
        module: module.path.into_boxed_slice(),
        siblings,
    })
}

/// The types the crate module at `path` (as [`locate`] takes it) brings
/// into scope by its `use` declarations, each by the name it binds and
/// with the traits its definition derives, wherever that definition is:
/// `iced_widget`'s `container` names `iced_core`'s `Border` through
/// `use crate::core::border::{self, Border}`, which derives `Default`.
/// The crate compiles those derives once, so the runner binds them as it
/// binds the module's own concrete items (`bound`). Empty when Cargo can't
/// describe the graph offline.
pub(crate) fn imported(manifest: &Path, path: &[ImmutableString]) -> Vec<super::bound::Derived> {
    let Some(graph) = Graph::read(manifest) else {
        return Vec::new();
    };
    let Some(module) = graph.module(path) else {
        return Vec::new();
    };
    let tokens = polyglot::placed(&module.text);
    let items = polyglot::module_items(&module.text, &tokens);
    let mut found = Vec::new();
    for import in imports(&tokens, &items) {
        let Bound::Name(name) = &import.bound else {
            continue;
        };
        let Some((last, parent)) = import.path.split_last() else {
            continue;
        };
        if parent.is_empty() {
            continue;
        }
        let Some(defined) = graph
            .resolve(&module, parent, HOPS)
            .and_then(|parent| graph.definition(&parent, last, HOPS))
        else {
            continue;
        };
        let spell = |word: &str| {
            if word == last.as_str() {
                return Some(name.clone());
            }
            graph.spelling(&module, &defined, word)
        };
        if let Some(derived) = super::bound::derived(&defined.text, last, &spell) {
            if std::env::var_os("RAWR_VERBOSE").is_some_and(|verbose| verbose == "1") {
                for (header, rule) in derived.excluded() {
                    eprintln!(
                        "rawr: `impl {header}` has no binding in `{}`: {rule}",
                        path.join("::")
                    );
                }
            }
            found.push(derived);
        }
    }
    found
}

/// The name of the program's hidden dependency on the crate `name`.
pub(crate) fn hidden_name(name: &str) -> ImmutableString {
    format!("{HIDDEN}{name}").into()
}

impl Located {
    /// The name of the program's dependency on the defining crate, which
    /// the module's `crate::` names: `__rawr_source_iced_widget`.
    pub(crate) fn alias(&self) -> Option<ImmutableString> {
        let library = self.package.library.as_ref()?;
        Some(hidden_name(&library.name))
    }

    /// The program's dependency on the defining crate, by [`Located::alias`]:
    /// the version or commit Cargo resolved, with no features of its own,
    /// so the graph Cargo resolves stays the program's, or the path of a
    /// path package the program reaches only through another crate. `None`
    /// for a path package the program names itself.
    pub(crate) fn dependency(&self) -> Option<Dependency> {
        let source = match self.package.source.as_deref() {
            Some(source) => source,
            // A path package the program reaches only through another
            // crate, as the fixture's `parts` behind `events`, by its path.
            None if !self.direct => {
                return self.hidden(Source::Path(self.package.directory.clone()));
            }
            None => return None,
        };
        let source = if source.starts_with("registry+") {
            Source::Registry {
                version: format!("={}", self.package.version),
            }
        } else {
            let (url, reference) = GitReference::parse_source(source)?;
            Source::Git {
                url: url.to_owned(),
                reference,
                version: None,
            }
        };
        self.hidden(source)
    }

    /// The program's hidden dependency on the defining crate at `source`.
    fn hidden(&self, source: Source) -> Option<Dependency> {
        let alias = self.alias()?;
        Some(Dependency {
            key: alias.to_string(),
            alias,
            package: Some(self.package.name.clone()),
            source,
            features: Box::default(),
            default_features: false,
            optional: false,
            span: crate::Span::NONE,
        })
    }
}

impl Package {
    /// The `[patch]` source a workspace root names this package's source
    /// by: `crates-io`, or its git repository's URL. `None` for a path
    /// package.
    pub(crate) fn patch_source(&self) -> Option<String> {
        let source = self.source.as_deref()?;
        if source.starts_with("registry+") {
            return Some("crates-io".to_owned());
        }
        let url = source.strip_prefix("git+")?;
        let end = url.find(['?', '#']).unwrap_or(url.len());
        Some(url[..end].to_owned())
    }

    /// The directory a copy of this package starts from: its git
    /// checkout's root, which holds the workspace its manifest inherits
    /// from, or the package's own directory.
    pub(crate) fn copy_root(&self) -> PathBuf {
        let components: Vec<_> = self.directory.components().collect();
        let checkouts = components
            .iter()
            .position(|component| component.as_os_str() == "checkouts");
        let git = self
            .source
            .as_deref()
            .is_some_and(|source| source.starts_with("git+"));
        match checkouts {
            Some(at) if git && components.len() > at + 2 => components[..at + 3].iter().collect(),
            _ => self.directory.clone(),
        }
    }
}

impl Graph {
    /// `cargo metadata` of the program whose manifest is `manifest`, read
    /// offline.
    fn read(manifest: &Path) -> Option<Self> {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args(["metadata", "--format-version", "1", "--offline"])
            .arg("--manifest-path")
            .arg(manifest)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
        let packages: Vec<Package> = json["packages"]
            .as_array()?
            .iter()
            .filter_map(package)
            .collect();
        let nodes: Vec<Node> = json["resolve"]["nodes"]
            .as_array()?
            .iter()
            .filter_map(node)
            .collect();
        Some(Self {
            packages,
            nodes,
            root: json["resolve"]["root"].as_str()?.to_owned(),
            texts: RefCell::new(BTreeMap::new()),
            children: RefCell::new(BTreeMap::new()),
        })
    }

    fn index(&self, id: &str) -> Option<usize> {
        self.packages.iter().position(|package| package.id == id)
    }

    fn node(&self, package: usize) -> Option<&Node> {
        let id = &self.packages[package].id;
        self.nodes.iter().find(|node| node.id == *id)
    }

    /// The package `from` names `name`.
    fn dependency(&self, from: usize, name: &str) -> Option<usize> {
        let edge = self
            .node(from)?
            .dependencies
            .iter()
            .find(|edge| edge.name == name)?;
        self.index(&edge.package)
    }

    /// The one package the program's graph holds whose library code names
    /// `name`, as `iced_core` behind `iced`'s re-exports, which hide its
    /// `widget::tree` from the program: where a crate module the program
    /// can't name is (#2698).
    fn reached(&self, name: &str) -> Option<usize> {
        let mut named = self.packages.iter().enumerate().filter(|(_, package)| {
            package
                .library
                .as_ref()
                .is_some_and(|library| library.name == name)
        });
        let (index, _) = named.next()?;
        named.next().is_none().then_some(index)
    }

    /// The features Cargo resolved for `package`.
    fn features(&self, package: usize) -> &[String] {
        self.node(package).map_or(&[], |node| &node.features)
    }

    /// The text of `file` of `package`, as its features select its items.
    fn text(&self, package: usize, file: &Path) -> Option<Rc<str>> {
        if let Some(text) = self.texts.borrow().get(file) {
            return Some(Rc::clone(text));
        }
        let features = Enabled::of_names(self.features(package).iter().map(String::as_str));
        let text: Rc<str> =
            polyglot::cfg_resolved(&std::fs::read_to_string(file).ok()?, &features).into();
        self.texts
            .borrow_mut()
            .insert(file.to_path_buf(), Rc::clone(&text));
        Some(text)
    }

    fn root_module(&self, package: usize) -> Option<Module> {
        let file = self.packages[package].library.as_ref()?.root.clone();
        Some(Module {
            package,
            path: Vec::new(),
            text: self.text(package, &file)?,
            directory: file.parent()?.to_path_buf(),
            file,
        })
    }

    /// The module `name` names in `module`: a child it declares, a module
    /// one of its `use` declarations binds to `name`, or one its globs
    /// bring in. `hops` bounds the re-exports followed.
    fn child(&self, module: &Module, name: &str, hops: usize) -> Option<Module> {
        let lookup = Lookup {
            package: module.package,
            path: module.path.clone(),
            name: name.to_owned(),
        };
        if let Some(known) = self.children.borrow().get(&lookup) {
            return known.clone();
        }
        self.children.borrow_mut().insert(lookup.clone(), None);
        let found = self.look_up(module, name, hops);
        self.children.borrow_mut().insert(lookup, found.clone());
        found
    }

    /// [`Graph::child`], before it is remembered.
    fn look_up(&self, module: &Module, name: &str, hops: usize) -> Option<Module> {
        let hops = hops.checked_sub(1)?;
        let tokens = polyglot::placed(&module.text);
        let items = polyglot::module_items(&module.text, &tokens);
        let declared = items
            .iter()
            .find(|item| item.keyword == "mod" && item.name == Some(name));
        if let Some(item) = declared {
            let mut path = module.path.clone();
            path.push(name.into());
            let directory = module.directory.join(name);
            if tokens[item.end].0 == Token::Punct(b'}') {
                let open = tokens[item.header.end].1 + 1;
                return Some(Module {
                    package: module.package,
                    path,
                    file: module.file.clone(),
                    text: module.text[open..tokens[item.end].1].into(),
                    directory,
                });
            }
            let flat = module.directory.join(format!("{name}.rs"));
            let file = if flat.is_file() {
                flat
            } else {
                directory.join("mod.rs")
            };
            return Some(Module {
                package: module.package,
                path,
                text: self.text(module.package, &file)?,
                file,
                directory,
            });
        }
        let imports = imports(&tokens, &items);
        let named = imports
            .iter()
            .find(|import| matches!(&import.bound, Bound::Name(bound) if bound == name));
        if let Some(import) = named {
            return self.resolve(module, &import.path, hops);
        }
        imports
            .iter()
            .filter(|import| matches!(import.bound, Bound::Glob))
            .filter_map(|import| self.resolve(module, &import.path, hops))
            .find_map(|globbed| self.child(&globbed, name, hops))
    }

    /// The module `path` names, written in `module`: from its crate's
    /// root (`crate`), from itself or its parents (`self`, `super`), from
    /// a dependency's root, or from a name it binds.
    fn resolve(&self, module: &Module, path: &[String], hops: usize) -> Option<Module> {
        let (first, rest) = path.split_first()?;
        let mut current = match first.as_str() {
            "crate" => self.root_module(module.package)?,
            "self" => module.clone(),
            "super" => self.parent(module)?,
            // Rust 2018 rejects a name that is both a dependency and an
            // item of the module, so the dependency is the one.
            _ => match self.dependency(module.package, first) {
                Some(package) => self.root_module(package)?,
                None => self.child(module, first, hops)?,
            },
        };
        for segment in rest {
            current = match segment.as_str() {
                "super" => self.parent(&current)?,
                "self" => current,
                _ => self.child(&current, segment, hops)?,
            };
        }
        Some(current)
    }

    /// The module at `path`, the program's path to it from a dependency's
    /// name ([`locate`]).
    fn module(&self, path: &[ImmutableString]) -> Option<Module> {
        let (first, rest) = path.split_first()?;
        let root = self.index(&self.root)?;
        let mut module = self.root_module(
            self.dependency(root, first)
                .or_else(|| self.reached(first))?,
        )?;
        for segment in rest {
            module = self.child(&module, segment, HOPS)?;
        }
        Some(module)
    }

    /// The module that defines the type `name` names in `module`: itself,
    /// or the one a `use` of `module` binding `name`, or one of its globs,
    /// reaches.
    fn definition(&self, module: &Module, name: &str, hops: usize) -> Option<Module> {
        let hops = hops.checked_sub(1)?;
        let tokens = polyglot::placed(&module.text);
        let items = polyglot::module_items(&module.text, &tokens);
        if items
            .iter()
            .any(|item| matches!(item.keyword, "struct" | "enum") && item.name == Some(name))
        {
            return Some(module.clone());
        }
        let imports = imports(&tokens, &items);
        let named = imports
            .iter()
            .find(|import| matches!(&import.bound, Bound::Name(bound) if bound == name));
        if let Some(import) = named {
            let (last, parent) = import.path.split_last()?;
            let parent = self.resolve(module, parent, hops)?;
            return self.definition(&parent, last, hops);
        }
        imports
            .iter()
            .filter(|import| matches!(import.bound, Bound::Glob))
            .filter_map(|import| self.resolve(module, &import.path, hops))
            .find_map(|globbed| self.definition(&globbed, name, hops))
    }

    /// The path by which `opening` writes `word`, a name the text of
    /// `defining` starts a path with, so both name one item: `word` itself
    /// where `opening` binds it to the item `defining` does, else a path
    /// through the crate that defines it (`crate_path`) or std's own.
    /// `None` when `opening` has no such path, as for a private item of
    /// another crate or a name only a glob brings in.
    fn spelling(&self, opening: &Module, defining: &Module, word: &str) -> Option<String> {
        match word {
            "crate" => return self.crate_path(opening, defining.package),
            "self" => return self.module_path(opening, defining),
            "super" => return self.module_path(opening, &self.parent(defining)?),
            _ => {}
        }
        let home = self.home(defining, word, HOPS);
        if home.is_some() && self.home(opening, word, HOPS) == home {
            return Some(word.to_owned());
        }
        let tokens = polyglot::placed(&defining.text);
        let items = polyglot::module_items(&defining.text, &tokens);
        let import = imports(&tokens, &items)
            .into_iter()
            .find(|import| matches!(&import.bound, Bound::Name(bound) if bound == word));
        let Some(import) = import else {
            // A path from std's crates, which every module names alike
            // unless it binds the name to an item of its own.
            if home.is_none() && matches!(word, "std" | "core" | "alloc") {
                return Some(word.to_owned());
            }
            // The defining module's own item.
            home.filter(|(package, path)| *package == defining.package && *path == defining.path)?;
            let module = self.module_path(opening, defining)?;
            return Some(format!("{module}::{word}"));
        };
        let (first, rest) = import.path.split_first()?;
        if matches!(first.as_str(), "std" | "core" | "alloc") {
            return Some(import.path.join("::"));
        }
        let start = match first.as_str() {
            "crate" => self.crate_path(opening, defining.package)?,
            "self" => self.module_path(opening, defining)?,
            "super" => self.module_path(opening, &self.parent(defining)?)?,
            _ => match self.dependency(defining.package, first) {
                Some(package) => self.crate_path(opening, package)?,
                None => format!("{}::{first}", self.module_path(opening, defining)?),
            },
        };
        Some(
            std::iter::once(start)
                .chain(rest.iter().cloned())
                .collect::<Vec<_>>()
                .join("::"),
        )
    }

    /// The path by which `opening` writes the module `module`.
    fn module_path(&self, opening: &Module, module: &Module) -> Option<String> {
        let root = self.crate_path(opening, module.package)?;
        Some(
            std::iter::once(root)
                .chain(module.path.iter().map(ToString::to_string))
                .collect::<Vec<_>>()
                .join("::"),
        )
    }

    /// The path by which `opening` writes the root of the package
    /// `package`: `crate` for its own, or `crate::alias` where its crate's
    /// root binds the package's root, as `iced_widget`'s `pub use
    /// iced_renderer::core` binds iced_core's through iced_renderer's own
    /// re-export, since an opened module reaches another crate only
    /// through its own crate's paths.
    fn crate_path(&self, opening: &Module, package: usize) -> Option<String> {
        if opening.package == package {
            return Some("crate".to_owned());
        }
        let root = self.root_module(opening.package)?;
        let tokens = polyglot::placed(&root.text);
        let items = polyglot::module_items(&root.text, &tokens);
        imports(&tokens, &items).into_iter().find_map(|import| {
            let Bound::Name(alias) = &import.bound else {
                return None;
            };
            self.resolve(&root, &import.path, HOPS)
                .filter(|module| module.package == package && module.path.is_empty())
                .map(|_| format!("crate::{alias}"))
        })
    }

    /// The package and module path of the module that defines the type
    /// or trait `name` names in `module` (as [`Graph::definition`]).
    fn home(
        &self,
        module: &Module,
        name: &str,
        hops: usize,
    ) -> Option<(usize, Vec<ImmutableString>)> {
        let hops = hops.checked_sub(1)?;
        let tokens = polyglot::placed(&module.text);
        let items = polyglot::module_items(&module.text, &tokens);
        if items.iter().any(|item| {
            matches!(item.keyword, "struct" | "enum" | "union" | "trait" | "type")
                && item.name == Some(name)
        }) {
            return Some((module.package, module.path.clone()));
        }
        let imports = imports(&tokens, &items);
        let named = imports
            .iter()
            .find(|import| matches!(&import.bound, Bound::Name(bound) if bound == name));
        if let Some(import) = named {
            let (last, parent) = import.path.split_last()?;
            let parent = self.resolve(module, parent, hops)?;
            return self.home(&parent, last, hops);
        }
        imports
            .iter()
            .filter(|import| matches!(import.bound, Bound::Glob))
            .filter_map(|import| self.resolve(module, &import.path, hops))
            .find_map(|globbed| self.home(&globbed, name, hops))
    }

    /// Whether the module that declares `module` declares it privately
    /// and re-exports its items by a public glob, `mod helpers; pub use
    /// helpers::*;`, so its items are its parent's to every other crate.
    fn globbed_privately(&self, module: &Module) -> bool {
        let (Some(name), Some(parent)) = (module.path.last(), self.parent(module)) else {
            return false;
        };
        let tokens = polyglot::placed(&parent.text);
        let items = polyglot::module_items(&parent.text, &tokens);
        let public = |item: &polyglot::ModuleItem<'_>| {
            item.header.start >= 2
                && matches!(
                    tokens[item.header.start - 2].0,
                    Token::Ident("pub", _) | Token::Punct(b')')
                )
        };
        let private = items
            .iter()
            .any(|item| item.keyword == "mod" && item.name == Some(name.as_str()) && !public(item));
        if !private {
            return false;
        }
        items
            .iter()
            .filter(|item| item.keyword == "use" && public(item))
            .any(|item| {
                imports(&tokens, std::slice::from_ref(item))
                    .iter()
                    .any(|import| {
                        matches!(import.bound, Bound::Glob)
                            && self
                                .resolve(&parent, &import.path, HOPS)
                                .is_some_and(|globbed| {
                                    globbed.package == module.package && globbed.path == module.path
                                })
                    })
            })
    }

    /// The module that declares `module`.
    fn parent(&self, module: &Module) -> Option<Module> {
        let (_, path) = module.path.split_last()?;
        let mut current = self.root_module(module.package)?;
        for segment in path {
            current = self.child(&current, segment, HOPS)?;
        }
        Some(current)
    }
}

/// A package of `cargo metadata`'s `packages`.
fn package(package: &serde_json::Value) -> Option<Package> {
    let manifest = PathBuf::from(package["manifest_path"].as_str()?);
    let library = package["targets"].as_array()?.iter().find_map(|target| {
        let kinds = target["kind"].as_array()?;
        let library = kinds
            .iter()
            .any(|kind| matches!(kind.as_str(), Some("lib" | "rlib" | "proc-macro")));
        if !library {
            return None;
        }
        Some(Library {
            name: target["name"].as_str()?.replace('-', "_"),
            root: PathBuf::from(target["src_path"].as_str()?),
        })
    });
    Some(Package {
        id: package["id"].as_str()?.to_owned(),
        name: package["name"].as_str()?.to_owned(),
        directory: manifest.parent()?.to_path_buf(),
        source: package["source"].as_str().map(str::to_owned),
        version: package["version"].as_str()?.to_owned(),
        library,
    })
}

/// A node of `cargo metadata`'s resolved graph.
fn node(node: &serde_json::Value) -> Option<Node> {
    let dependencies: Vec<Edge> = node["deps"]
        .as_array()?
        .iter()
        .filter_map(|edge| {
            Some(Edge {
                name: edge["name"].as_str()?.to_owned(),
                package: edge["pkg"].as_str()?.to_owned(),
            })
        })
        .collect();
    let features: Vec<String> = node["features"]
        .as_array()?
        .iter()
        .filter_map(|feature| Some(feature.as_str()?.to_owned()))
        .collect();
    Some(Node {
        id: node["id"].as_str()?.to_owned(),
        dependencies,
        features,
    })
}

/// Every import the `use` declarations of `items` make.
fn imports(tokens: &[(Token<'_>, usize)], items: &[polyglot::ModuleItem<'_>]) -> Vec<Import> {
    let mut imports = Vec::new();
    for item in items.iter().filter(|item| item.keyword == "use") {
        let tree: Vec<Token<'_>> = tokens[item.header.start..item.end]
            .iter()
            .map(|(token, _)| *token)
            .collect();
        let mut cursor = 0;
        tree_imports(&tree, &mut cursor, &[], &mut imports);
    }
    imports
}

/// The imports of the use tree at `cursor` of `tree`, under `prefix`, past
/// it: `a::b`, `a::b as c`, `a::*` and `a::{..}`.
fn tree_imports(
    tree: &[Token<'_>],
    cursor: &mut usize,
    prefix: &[String],
    imports: &mut Vec<Import>,
) {
    let mut path = prefix.to_vec();
    loop {
        match tree.get(*cursor) {
            Some(Token::Punct(b':')) => *cursor += 1,
            Some(Token::Ident("as", _)) => {
                if let Some(Token::Ident(name, _)) = tree.get(*cursor + 1) {
                    imports.push(Import {
                        path,
                        bound: Bound::Name((*name).to_owned()),
                    });
                }
                *cursor += 2;
                return;
            }
            Some(Token::Ident(segment, _)) => {
                path.push((*segment).to_owned());
                *cursor += 1;
            }
            Some(Token::Punct(b'*')) => {
                *cursor += 1;
                imports.push(Import {
                    path,
                    bound: Bound::Glob,
                });
                return;
            }
            Some(Token::Punct(b'{')) => {
                *cursor += 1;
                loop {
                    match tree.get(*cursor) {
                        Some(Token::Punct(b'}')) | None => {
                            *cursor += 1;
                            return;
                        }
                        Some(Token::Punct(b',')) => *cursor += 1,
                        _ => tree_imports(tree, cursor, &path, imports),
                    }
                }
            }
            _ => {
                // `a::{self}` binds `a`.
                if path.last().is_some_and(|last| last == "self") {
                    path.pop();
                }
                if let Some(name) = path.last().cloned() {
                    imports.push(Import {
                        path,
                        bound: Bound::Name(name),
                    });
                }
                return;
            }
        }
    }
}
