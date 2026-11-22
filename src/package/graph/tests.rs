//! What native code sees of marked modules, read from real crates: the
//! private items a native module below a marked one names, and the shape
//! that decides whether a save swaps or restarts.

use std::fs;
use std::path::{Path, PathBuf};

use super::Graph;
use crate::package::island::Seen;
use crate::{project, Engine, ImmutableString};

/// A copy of the import CLI's test crate `name` under the build directory,
/// removed when it drops.
struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str, copy: &str) -> Self {
        let from = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("import-cli/tests/fixtures")
            .join(name);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/graph-tests")
            .join(format!("{copy}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        copy_tree(&from, &root);
        Self {
            root: fs::canonicalize(root).unwrap(),
        }
    }

    fn graph(&self) -> Graph {
        let mut answers = crate::project::cargo::Answers::new();
        let target = project::Target::discover(&self.root, &mut answers).unwrap();
        Graph::load(&Engine::new(), &target, &mut answers).unwrap()
    }

    fn edit(&self, path: &str, from: &str, to: &str) {
        let path = self.root.join(path);
        let text = fs::read_to_string(&path).unwrap();
        let edited = text.replace(from, to);
        assert_ne!(edited, text, "{from:?} is not in {}", path.display());
        fs::write(path, edited).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A native module below a marked one that names none of its items sees
/// none of its private ones: a new private field of the marked module's
/// public type, which native code holds but never opens, leaves the shape
/// native code sees as it was, so the save swaps (#2282).
#[cfg(all(feature = "rust-import", not(target_arch = "wasm32")))]
#[test]
fn a_private_field_no_native_module_names_leaves_the_shape() {
    use crate::package::called;

    let fixture = Fixture::new("islands-theme", "private-field");
    let rawr = fixture.root.join("target/rawr");
    fs::create_dir_all(&rawr).unwrap();
    // What a check of the crate records: native code holds the screen's
    // state but never opens it.
    called::store_in(
        &rawr,
        &called::Used {
            named: &std::collections::BTreeSet::new(),
            opaque: &["home::State".to_owned()].into(),
        },
    )
    .unwrap();
    let shape = || {
        let mut graph = fixture.graph();
        graph.keep_rawr_directory(rawr.clone());
        (graph.island_shape().unwrap(), graph)
    };
    let (before, graph) = shape();
    let home = [ImmutableString::from("home")];
    let seen = graph.entry_package().below.get(&home[..]);
    assert!(seen.is_none_or(|seen| !seen.contains("State")), "{seen:?}");
    fixture.edit(
        "src/home.rs",
        "    wizard: Option<wizard::State>,\n}",
        "    wizard: Option<wizard::State>,\n    seen: u8,\n}",
    );
    fixture.edit(
        "src/home.rs",
        "            wizard: None,\n        }",
        "            wizard: None,\n            seen: 0,\n        }",
    );
    let (after, _) = shape();
    assert_eq!(before.change(&after), None);
}

/// A native module below a marked root sees the private items its paths
/// name, `super::scale` and `super::Reading`, and what their declarations
/// name, and no other: each it sees gets its trampoline or its copy, with
/// its fields.
#[test]
fn a_marked_roots_native_module_sees_the_private_items_it_names() {
    let fixture = Fixture::new("marked-root-native", "named-items");
    let graph = fixture.graph();
    let seen = graph.entry_package().below.get(&[][..]).unwrap();
    assert_eq!(
        *seen,
        Seen::Names(["Reading", "scale"].map(str::to_owned).into())
    );
    let shadow = graph.island_shadow_for(None).unwrap();
    let [(path, text)] = shadow.files.as_slice() else {
        panic!("{:?}", shadow.files);
    };
    assert_eq!(path, Path::new("main.rs"));
    assert!(
        text.contains("struct Reading {\n    tick: i64,\n    value: i64,\n}"),
        "{text}"
    );
    assert!(
        text.contains("crate::__rawr_island::call(\"scale\", false);"),
        "{text}"
    );
    assert!(!text.contains("SCALE"), "{text}");
}

/// A file's inline modules are sources at the file's own path, and its
/// snapshot holds the file once, in which the snapshot's load finds the
/// inline modules again (#2344). The file beside it is no module of it.
#[test]
fn a_snapshot_holds_a_file_with_inline_modules_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/graph-tests")
        .join(format!("inline-snapshot-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let file = root.join("main.rs");
    let text = "mod outer { pub mod inner { pub fn f() -> i32 { 3 } } }\n\
                fn main() { println!(\"{}\", outer::inner::f()); }\n";
    fs::write(&file, text).unwrap();
    fs::write(root.join("other.rs"), "fn main() {}\n").unwrap();
    let mut answers = crate::project::cargo::Answers::new();
    let target = project::Target::discover(&file, &mut answers).unwrap();
    let graph = Graph::load(&Engine::new(), &target, &mut answers).unwrap();
    assert_eq!(graph.sources().len(), 3);
    let snapshot = crate::package::snapshot::Snapshot::of(&graph).unwrap();
    assert_eq!(snapshot.read(snapshot.entry()), Some(text));
    let reloaded = Graph::load_snapshot(
        &Engine::new(),
        &snapshot,
        &mut crate::project::cargo::Answers::new(),
    )
    .unwrap();
    assert_eq!(reloaded.sources().len(), 3);
    let _ = fs::remove_dir_all(&root);
}

/// A `macro_rules!` a module defines before its `mod` items is in scope in
/// the files of those modules and the files below them, as rustc's textual
/// scope has it, where an item goes and in a body (#2190). The output is
/// rustc's.
#[test]
fn a_macro_defined_before_a_mod_is_in_scope_in_its_files() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/graph-tests")
        .join(format!("macro-scope-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("src/a/b")).unwrap();
    let files = [
        (
            "Cargo.toml",
            "[package]\nname = \"scope\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
        ),
        (
            "src/main.rs",
            "// rawr\nmacro_rules! one { () => { 1 }; }\n\
             macro_rules! make { ($name:ident, $value:expr) => { pub fn $name() -> i32 { $value } }; }\n\
             mod a;\n\
             fn main() { println!(\"{} {} {} {}\", a::first(), a::b::two(), a::b::c::three(), a::made()); }\n",
        ),
        ("src/a.rs", "// rawr\npub mod b;\nmake!(made, 4);\npub fn first() -> i32 { one!() }\n"),
        (
            "src/a/b.rs",
            "// rawr\nmacro_rules! two_of { () => { one!() + one!() }; }\npub mod c;\n\
             pub fn two() -> i32 { two_of!() }\n",
        ),
        ("src/a/b/c.rs", "// rawr\npub fn three() -> i32 { two_of!() + one!() }\n"),
    ];
    for (path, text) in files {
        fs::write(root.join(path), text).unwrap();
    }
    let mut answers = crate::project::cargo::Answers::new();
    let target = project::Target::discover(&root, &mut answers).unwrap();
    let graph = Graph::load(&Engine::new(), &target, &mut answers).unwrap();
    let printed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let mut engine = Engine::new();
    let sink = std::sync::Arc::clone(&printed);
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    let (ast, main) = engine
        .compile_package(graph)
        .unwrap_or_else(|error| panic!("{error}"));
    let mut scope = crate::Scope::new();
    let _: crate::Dynamic = engine
        .eval_ast_with_scope::<crate::Dynamic>(&mut scope, &ast)
        .unwrap();
    let main = main.expect("the package has a main");
    let _: crate::Dynamic = engine
        .call_fn_with_options::<crate::Dynamic>(
            crate::CallFnOptions::new()
                .eval_ast(false)
                .rewind_scope(false),
            &mut scope,
            &ast,
            main,
            (),
        )
        .unwrap();
    assert_eq!(*printed.lock().unwrap(), "1 2 3 4\n");
    let _ = fs::remove_dir_all(&root);
}
