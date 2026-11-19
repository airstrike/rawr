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
        let target = project::Target::discover(&self.root).unwrap();
        Graph::load(&Engine::new(), &target).unwrap()
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
