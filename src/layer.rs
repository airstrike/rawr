//! Layers over the base language. Every program gets the base: the language
//! and its core intrinsics (`format!`, `println!`, ..). A layer (rawr-iced is
//! the first) adds a domain on top when a program brings it in; each layer
//! says itself what brings it in, from the [`Facts`] of the program.

use crate::item::{self, type_ref};
use crate::{diagnostic, package, ImmutableString};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// What a host knows about a program from loading its package graph, before
/// any layer's work: derived from the parse that loading already did, never
/// by reading or lexing the sources again.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    roots: BTreeSet<ImmutableString>,
    rust_import: bool,
}

/// A layer a host has installed: its name and what brings it in.
#[derive(Clone, Copy, Debug)]
pub struct Layer {
    /// The crate providing the layer.
    pub name: &'static str,
    /// Whether the program brings this layer in: a pure function of its facts.
    pub claims: fn(&Facts) -> bool,
}

/// A program's [`Facts`] kept across reloads, spreadsheet style: one cell per
/// source file, keyed by its path and computed again only when its text
/// changed; the facts are recomputed from the cells only when a cell did.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    cells: BTreeMap<PathBuf, Cell>,
    facts: Facts,
}

#[derive(Clone, Debug)]
struct Cell {
    /// The text this cell was computed from.
    source: diagnostic::Source,
    roots: BTreeSet<ImmutableString>,
}

/// What one [`Sheet::update`] did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Update {
    /// How many sources had their facts computed: new or edited files.
    pub recomputed: usize,
    /// Whether the program's facts changed, so layers must judge it again.
    pub changed: bool,
}

impl Facts {
    /// The facts of a loaded package graph.
    #[must_use]
    pub fn of(graph: &package::Graph) -> Self {
        let mut sheet = Sheet::default();
        sheet.update(graph);
        sheet.facts
    }

    /// Whether the package declares `[rust]` imports.
    #[must_use]
    pub const fn declares_rust_import(&self) -> bool {
        self.rust_import
    }

    /// Whether an item names the crate `name`: `use name..`, or a path
    /// starting `name::` in a signature or a body.
    #[must_use]
    pub fn names_crate(&self, name: &str) -> bool {
        self.roots.contains(name)
    }
}

impl Sheet {
    /// The facts as of the last update.
    #[must_use]
    pub const fn facts(&self) -> &Facts {
        &self.facts
    }

    /// Bring the sheet to a newly loaded graph of the same program: a file
    /// whose text is unchanged keeps its cell; a new or edited one is
    /// computed from the items loading parsed; a file gone drops its cell.
    pub fn update(&mut self, graph: &package::Graph) -> Update {
        let mut recomputed = 0;
        let mut cells = BTreeMap::new();
        for (source, items) in graph.parsed_sources() {
            let cell = match self.cells.remove(source.path()) {
                Some(cell) if cell.source.text() == source.text() => cell,
                _ => {
                    recomputed += 1;
                    let mut roots = BTreeSet::new();
                    for parsed in items {
                        parsed_roots(parsed, &mut roots);
                    }
                    Cell {
                        source: source.diagnostic().clone(),
                        roots,
                    }
                }
            };
            cells.insert(source.path().to_path_buf(), cell);
        }
        let dropped = !self.cells.is_empty();
        self.cells = cells;
        #[cfg(feature = "rust-import")]
        let rust_import = graph.declares_rust_import();
        #[cfg(not(feature = "rust-import"))]
        let rust_import = false;
        let mut changed = rust_import != self.facts.rust_import;
        self.facts.rust_import = rust_import;
        if recomputed > 0 || dropped {
            let roots = self
                .cells
                .values()
                .flat_map(|cell| cell.roots.iter().cloned())
                .collect();
            changed |= roots != self.facts.roots;
            self.facts.roots = roots;
        }
        Update {
            recomputed,
            changed,
        }
    }
}

/// The first segment of every path `parsed` writes.
fn parsed_roots(parsed: &item::Parsed, roots: &mut BTreeSet<ImmutableString>) {
    match parsed {
        item::Parsed::Use(item) => {
            let (item::UseTree::Path(path, _)
            | item::UseTree::Group { prefix: path, .. }
            | item::UseTree::Glob { prefix: path, .. }) = &item.tree;
            // `use iced;` names the crate with its only segment.
            if let Some(first) = path.segments.first() {
                roots.insert(first.name.text.clone());
            }
        }
        item::Parsed::Function(function) => function_roots(function, roots),
        item::Parsed::LegacyFunction(function) => body_roots(&function.body_syntax, roots),
        item::Parsed::Struct(item) => {
            for field in &item.fields {
                type_roots(&field.ty, roots);
            }
        }
        item::Parsed::Enum(item) => {
            for ty in item.variants.iter().flat_map(|variant| &variant.payload) {
                type_roots(ty, roots);
            }
        }
        item::Parsed::Impl(item) => {
            if let Some(path) = &item.trait_ {
                path_roots(path, roots);
            }
            type_roots(&item.owner, roots);
            for function in &item.functions {
                function_roots(function, roots);
            }
        }
        item::Parsed::Trait(item) => {
            for function in &item.functions {
                function_roots(function, roots);
            }
        }
        item::Parsed::Module(_) => {}
    }
}

fn function_roots(function: &item::Function, roots: &mut BTreeSet<ImmutableString>) {
    for parameter in &function.parameters {
        type_roots(&parameter.ty, roots);
    }
    if let Some(output) = &function.output {
        type_roots(output, roots);
    }
    body_roots(&function.body_syntax, roots);
}

/// `name::` not after another `::`.
fn body_roots(body: &item::BodySyntax, roots: &mut BTreeSet<ImmutableString>) {
    let tokens = body.tokens();
    for (index, pair) in tokens.windows(2).enumerate() {
        if let (item::BodyTokenKind::Identifier(name), item::BodyTokenKind::DoubleColon) =
            (pair[0].kind(), pair[1].kind())
            && !(index > 0 && matches!(tokens[index - 1].kind(), item::BodyTokenKind::DoubleColon))
        {
            roots.insert(name.clone());
        }
    }
}

fn type_roots(ty: &type_ref::Type, roots: &mut BTreeSet<ImmutableString>) {
    match ty {
        type_ref::Type::Unit(_) => {}
        type_ref::Type::Tuple { members, .. } => {
            for member in members {
                type_roots(member, roots);
            }
        }
        type_ref::Type::Path(path) => path_roots(path, roots),
        type_ref::Type::Reference { target, .. } => type_roots(target, roots),
    }
}

fn path_roots(path: &type_ref::Path, roots: &mut BTreeSet<ImmutableString>) {
    if let [first, _, ..] = &path.segments[..] {
        roots.insert(first.name.text.clone());
    }
    for argument in path.segments.iter().flat_map(|segment| &segment.arguments) {
        if let type_ref::Argument::Type(ty) = argument {
            type_roots(ty, roots);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Facts;

    fn facts(source: &str) -> Facts {
        let id = std::process::id();
        let path =
            std::env::temp_dir().join(format!("rawr-layer-facts-{id}-{}.rawr", source.len()));
        std::fs::write(&path, source).unwrap();
        let target = crate::project::Target::discover(&path).unwrap();
        let graph = crate::package::Graph::load(&crate::Engine::new(), &target).unwrap();
        std::fs::remove_file(path).unwrap();
        Facts::of(&graph)
    }

    #[test]
    fn a_crate_is_named_by_a_use_or_a_path() {
        for source in [
            "use iced::widget::text;",
            "use iced;",
            "pub fn main() -> iced::Result { Ok(()) }",
            "struct S { color: Option<iced::Color> }",
            "fn f(n: i64) -> i64 { let x = iced::Length::Fill; n }",
        ] {
            assert!(facts(source).names_crate("iced"), "{source}");
        }
        for source in [
            "fn f(iced: i64) -> i64 { iced }",
            "// use iced::widget;\nfn f(n: i64) -> i64 { n }",
            "enum E { A } fn f(n: i64) -> i64 { let e = E::A; n }",
            "fn main() { println!(\"iced::x\"); }",
        ] {
            assert!(!facts(source).names_crate("iced"), "{source}");
        }
    }

    #[test]
    fn a_reload_recomputes_only_the_edited_files_facts() {
        let root = std::env::temp_dir().join(format!("rawr-layer-sheet-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let main = "mod view;\nfn main() { let n = view::count(); }\n";
        std::fs::write(root.join("src/main.rawr"), main).unwrap();
        let view = |text: &str| std::fs::write(root.join("src/view.rawr"), text).unwrap();
        let mut sheet = super::Sheet::default();
        let load = |sheet: &mut super::Sheet| {
            let target = crate::project::Target::discover(&root).unwrap();
            sheet.update(&crate::package::Graph::load(&crate::Engine::new(), &target).unwrap())
        };
        let update = |recomputed, changed| super::Update {
            recomputed,
            changed,
        };

        view("pub fn count() -> i64 { 1 }\n");
        assert_eq!(load(&mut sheet), update(2, true));
        assert!(sheet.facts().names_crate("view"));
        // Nothing saved: no cell is computed.
        assert_eq!(load(&mut sheet), update(0, false));
        // An edit naming no new crate: that one file, same facts.
        view("pub fn count() -> i64 { 2 }\n");
        assert_eq!(load(&mut sheet), update(1, false));
        // Bringing iced in, then taking it out again.
        view("use iced::widget::text;\npub fn count() -> i64 { 2 }\n");
        assert_eq!(load(&mut sheet), update(1, true));
        assert!(sheet.facts().names_crate("iced"));
        view("pub fn count() -> i64 { 2 }\n");
        assert_eq!(load(&mut sheet), update(1, true));
        assert!(!sheet.facts().names_crate("iced"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
