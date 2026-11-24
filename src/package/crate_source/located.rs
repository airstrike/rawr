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
    let (first, rest) = path.split_first()?;
    let root = graph.index(&graph.root)?;
    let mut module = graph.root_module(graph.dependency(root, first)?)?;
    for segment in rest {
        module = graph.child(&module, segment, HOPS)?;
    }
    if module.path.is_empty() {
        return None;
    }
    let package = graph.packages[module.package].clone();
    let siblings: Vec<Package> = graph
        .packages
        .iter()
        .filter(|other| other.source.is_some() && other.source == package.source)
        .cloned()
        .collect();
    Some(Located {
        features: graph.features(module.package).to_vec(),
        file: module.file,
        package,
        module: module.path.into_boxed_slice(),
        siblings,
    })
}

impl Located {
    /// The name of the program's dependency on the defining crate, which
    /// the module's `crate::` names: `__rawr_source_iced_widget`.
    pub(crate) fn alias(&self) -> Option<ImmutableString> {
        let library = self.package.library.as_ref()?;
        Some(format!("{HIDDEN}{}", library.name).into())
    }

    /// The program's dependency on the defining crate, by [`Located::alias`]:
    /// the version or commit Cargo resolved, with no features of its own,
    /// so the graph Cargo resolves stays the program's. `None` for a path
    /// package, which the program names itself.
    pub(crate) fn dependency(&self) -> Option<Dependency> {
        let source = self.package.source.as_deref()?;
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
