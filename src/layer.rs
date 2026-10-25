//! Extensions over the base language. Every program gets the base: the
//! language and its core intrinsics (`format!`, `println!`, ..). An
//! extension (an installed `rawr-<name>` binary) serves a program that
//! brings it in; each extension says itself what brings it in, from the
//! [`Facts`] of the program.

use crate::item::{self, type_ref};
use crate::{diagnostic, package, parse, ImmutableString};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

/// What a host knows about a program from loading its package graph, before
/// any layer's work: derived from the parse that loading already did, never
/// by reading or lexing the sources again.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Facts {
    roots: BTreeSet<ImmutableString>,
    free: BTreeSet<ImmutableString>,
    rust_import: bool,
}

/// What brings a layer in, as the layer declares it: a program naming one
/// of `crates`, or whose package manifest declares one of `sections`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Claims {
    /// Crates whose use brings the extension in.
    pub crates: Vec<String>,
    /// Kinds of import that bring the extension in. Only `rust` exists: a
    /// crate declares it with Rust dependencies in its `Cargo.toml` or with
    /// native modules.
    pub sections: Vec<String>,
    /// Paths a host may import for a snippet that writes no `use` of its
    /// own. Only such a host (`?play`)
    /// reads them, through [`Claims::claim_snippet`]; `rawr` does not.
    pub prelude: Vec<String>,
}

impl Claims {
    /// Whether a program with these facts brings the layer in.
    #[must_use]
    pub fn claim(&self, facts: &Facts) -> bool {
        self.crates.iter().any(|name| facts.names_crate(name))
            || self.sections.iter().any(|section| facts.declares(section))
    }

    /// Whether a snippet with these facts brings the layer in, for a host
    /// that imports [`Claims::prelude`] into a snippet: [`Claims::claim`], or
    /// a prelude name the snippet uses and neither declares nor imports.
    #[must_use]
    pub fn claim_snippet(&self, facts: &Facts) -> bool {
        self.claim(facts)
            || self.prelude.iter().any(|path| {
                let name = path.rsplit("::").next().unwrap_or(path);
                facts.uses_free(name)
            })
    }

    /// The `use` items that import [`Claims::prelude`], on one line.
    #[must_use]
    pub fn prelude_imports(&self) -> String {
        self.prelude
            .iter()
            .map(|path| format!("use {path}; "))
            .collect()
    }

    /// What in `facts` brings the extension in, as a user reads it: a crate
    /// quoted (`` `name` ``) or `Rust dependencies in Cargo.toml`.
    #[must_use]
    pub fn reason(&self, facts: &Facts) -> Option<String> {
        self.crates
            .iter()
            .find(|name| facts.names_crate(name))
            .map(|name| format!("`{name}`"))
            .or_else(|| {
                self.sections
                    .iter()
                    .find(|section| facts.declares(section))
                    .map(|section| section_reason(section))
            })
    }
}

/// A section that brings a layer in, as a user reads it.
#[must_use]
pub fn section_reason(section: &str) -> String {
    if section == "rust" {
        "Rust dependencies in Cargo.toml".to_owned()
    } else {
        format!("`{section}` in Cargo.toml")
    }
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
    names: Names,
}

/// The names one source writes.
#[derive(Clone, Debug, Default)]
struct Names {
    /// The first segment of every path: the crates and modules it names.
    roots: BTreeSet<ImmutableString>,
    /// Every name it calls, invokes as a macro, starts a path with or
    /// writes as a type.
    used: BTreeSet<ImmutableString>,
    /// Every name its items declare or its `use` items bind.
    declared: BTreeSet<ImmutableString>,
}

impl Names {
    fn of(items: &[item::Parsed]) -> Self {
        let mut names = Self::default();
        for parsed in items {
            names.parsed(parsed);
        }
        names
    }
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

    /// The facts of one parsed in-memory source, such as a snippet: the
    /// parse a host judges is the parse it compiles.
    #[must_use]
    pub fn of_unit(unit: &parse::Unit) -> Self {
        let names = Names::of(unit.items());
        Self {
            free: names.used.difference(&names.declared).cloned().collect(),
            roots: names.roots,
            rust_import: false,
        }
    }

    /// Whether the crate imports Rust: Cargo dependencies or native modules.
    #[must_use]
    pub const fn declares_rust_import(&self) -> bool {
        self.rust_import
    }

    /// Whether the crate declares the imports `section` names. Only `rust`
    /// exists: Rust dependencies or native modules.
    #[must_use]
    pub fn declares(&self, section: &str) -> bool {
        section == "rust" && self.rust_import
    }

    /// Whether an item names the crate `name`: `use name..`, or a path
    /// starting `name::` in a signature or a body.
    #[must_use]
    pub fn names_crate(&self, name: &str) -> bool {
        self.roots.contains(name)
    }

    /// Whether an item uses `name` (calls it, invokes it as a macro, starts
    /// a path or writes a type with it) and no item declares or imports it.
    #[must_use]
    pub fn uses_free(&self, name: &str) -> bool {
        self.free.contains(name)
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
            // An inline module shares its file's path: its module path
            // tells the two apart.
            let mut key = source.path().as_os_str().to_owned();
            for module in source.modules() {
                key.push("#");
                key.push(module.as_str());
            }
            let key = PathBuf::from(key);
            let cell = match self.cells.remove(&key) {
                Some(cell) if cell.source.text() == source.text() => cell,
                _ => {
                    recomputed += 1;
                    Cell {
                        source: source.diagnostic().clone(),
                        names: Names::of(items),
                    }
                }
            };
            cells.insert(key, cell);
        }
        let dropped = !self.cells.is_empty();
        self.cells = cells;
        let rust_import = graph.declares_rust_import();
        let mut changed = rust_import != self.facts.rust_import;
        self.facts.rust_import = rust_import;
        if recomputed > 0 || dropped {
            let union = |set: fn(&Names) -> &BTreeSet<ImmutableString>| -> BTreeSet<_> {
                self.cells
                    .values()
                    .flat_map(|cell| set(&cell.names).iter().cloned())
                    .collect()
            };
            let roots = union(|names| &names.roots);
            let declared = union(|names| &names.declared);
            let free = union(|names| &names.used)
                .difference(&declared)
                .cloned()
                .collect();
            changed |= roots != self.facts.roots || free != self.facts.free;
            self.facts.roots = roots;
            self.facts.free = free;
        }
        Update {
            recomputed,
            changed,
        }
    }
}

impl Names {
    /// The names `parsed` writes and declares.
    fn parsed(&mut self, parsed: &item::Parsed) {
        match parsed {
            item::Parsed::Use(item) => {
                let (item::UseTree::Path(path, _)
                | item::UseTree::Group { prefix: path, .. }
                | item::UseTree::Glob { prefix: path, .. }) = &item.tree;
                // `use iced;` names the crate with its only segment.
                if let Some(first) = path.segments.first() {
                    self.roots.insert(first.name.text.clone());
                }
                for (name, _) in item.bindings() {
                    self.declared.insert(name.text);
                }
            }
            item::Parsed::Function(function) => {
                self.declared.insert(function.name.text.clone());
                self.function(function);
            }
            item::Parsed::Struct(item) => {
                self.declared.insert(item.name.text.clone());
                for field in &item.fields {
                    self.ty(&field.ty);
                }
            }
            item::Parsed::Enum(item) => {
                self.declared.insert(item.name.text.clone());
                for ty in item.variants.iter().flat_map(|variant| &variant.payload) {
                    self.ty(ty);
                }
            }
            item::Parsed::Impl(item) => {
                if let Some(path) = &item.trait_ {
                    self.path(path);
                }
                self.ty(&item.owner);
                for function in &item.functions {
                    self.function(function);
                }
            }
            item::Parsed::Trait(item) => {
                self.declared.insert(item.name.text.clone());
                for function in &item.functions {
                    self.function(function);
                }
            }
            item::Parsed::Module(item) => {
                self.declared.insert(item.name.text.clone());
            }
            item::Parsed::TypeAlias(item) => {
                self.declared.insert(item.name.text.clone());
                self.ty(&item.target);
            }
        }
    }

    fn function(&mut self, function: &item::Function) {
        for parameter in &function.parameters {
            self.ty(&parameter.ty);
        }
        if let Some(output) = &function.output {
            self.ty(output);
        }
        self.body(&function.body_syntax);
    }

    /// `name::` starts a path, `name(` calls and `name!` invokes a macro,
    /// each not after `::` or `.`.
    fn body(&mut self, body: &item::BodySyntax) {
        use item::BodyTokenKind as Kind;
        let tokens = body.tokens();
        for (index, pair) in tokens.windows(2).enumerate() {
            let Kind::Identifier(name) = pair[0].kind() else {
                continue;
            };
            if index > 0 && matches!(tokens[index - 1].kind(), Kind::DoubleColon | Kind::Dot) {
                continue;
            }
            match pair[1].kind() {
                Kind::DoubleColon => {
                    self.roots.insert(name.clone());
                    self.used.insert(name.clone());
                }
                Kind::LeftParen | Kind::Unit | Kind::Bang => {
                    self.used.insert(name.clone());
                }
                _ => {}
            }
        }
    }

    fn ty(&mut self, ty: &type_ref::Type) {
        match ty {
            type_ref::Type::Unit(_) => {}
            type_ref::Type::Tuple { members, .. } => {
                for member in members {
                    self.ty(member);
                }
            }
            type_ref::Type::Path(path) => self.path(path),
            type_ref::Type::Reference { target, .. } => self.ty(target),
        }
    }

    fn path(&mut self, path: &type_ref::Path) {
        if let Some(first) = path.segments.first() {
            if path.segments.len() > 1 {
                self.roots.insert(first.name.text.clone());
            }
            self.used.insert(first.name.text.clone());
        }
        for argument in path.segments.iter().flat_map(|segment| &segment.arguments) {
            if let type_ref::Argument::Type(ty) = argument {
                self.ty(ty);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Facts;

    fn facts(source: &str) -> Facts {
        let id = std::process::id();
        let path = std::env::temp_dir().join(format!("rawr-layer-facts-{id}-{}.rs", source.len()));
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
    fn a_snippet_uses_the_names_it_neither_declares_nor_imports() {
        let unit = |source: &str| Facts::of_unit(&crate::Engine::new().parse_unit(source).unwrap());
        let facts = unit(
            "fn main() { println!(\"{:?}\", column![text(\"hi\"), helper()].padding(Pad::new())); }\n\
             fn helper() -> i64 { let n = 1; n.text() }\nstruct Pad;",
        );
        for name in ["column", "text", "println"] {
            assert!(facts.uses_free(name), "{name}");
        }
        // Declared, a local, a method.
        for name in ["helper", "Pad", "n", "padding"] {
            assert!(!facts.uses_free(name), "{name}");
        }
        let imported = unit("use iced::widget::text;\nfn main() { let t = text(\"x\"); }");
        assert!(!imported.uses_free("text"));
        assert!(imported.names_crate("iced"));
        let typed = unit("fn view() -> Element<()> { 1 }");
        assert!(typed.uses_free("Element"));
    }

    #[test]
    fn a_snippet_host_claims_by_crate_or_by_prelude_name() {
        let claims = super::Claims {
            crates: vec!["gui".into()],
            sections: Vec::new(),
            prelude: vec!["gui::widget::label".into()],
        };
        let unit = |source: &str| Facts::of_unit(&crate::Engine::new().parse_unit(source).unwrap());
        let named = unit("fn main() { let x = gui::widget::label(\"a\"); }");
        assert!(claims.claim(&named) && claims.claim_snippet(&named));
        let prelude = unit("fn main() { let x = label(\"a\"); }");
        assert!(!claims.claim(&prelude), "rawr has no prelude");
        assert!(claims.claim_snippet(&prelude));
        let own = unit("fn label(s: &str) -> i64 { 1 }\nfn main() { let x = label(\"a\"); }");
        assert!(!claims.claim_snippet(&own));
        let plain = unit("fn main() { println!(\"{}\", 1 + 2); }");
        assert!(!claims.claim_snippet(&plain));
        assert_eq!(claims.prelude_imports(), "use gui::widget::label; ");
    }

    #[test]
    fn a_reload_recomputes_only_the_edited_files_facts() {
        let root = std::env::temp_dir().join(format!("rawr-layer-sheet-{}", std::process::id()));
        std::fs::create_dir_all(root.join("src")).unwrap();
        let main = "// rawr\nmod view;\nfn main() { let n = view::count(); }\n";
        std::fs::write(root.join("Cargo.toml"), "[package]\nname = \"sheet\"\n").unwrap();
        std::fs::write(root.join("src/main.rs"), main).unwrap();
        let view = |text: &str| {
            std::fs::write(root.join("src/view.rs"), format!("// rawr\n{text}")).unwrap();
        };
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
