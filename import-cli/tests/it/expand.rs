//! A crate derive's expansion for a program's type (#593): rustc expands a
//! small crate of the item against the artifacts the project's resolution
//! builds, and the produced items come back as source text, kept in the
//! project's `target/rawr/expand/`. The fixture's `serde` is real serde.

use std::fs;
use std::path::Path;
use std::process::Command;

const DERIVES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/plain-derives");

/// The child's marker: the work runs in a child of this test's binary,
/// whose `RAWR_HOME` is the suite's, as a test sets no variable of its own
/// process.
const CHILD: &str = "RAWR_TEST_EXPAND_CHILD";

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy(&path, &to.join(entry.file_name()));
        } else {
            fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn a_crates_derive_expands_through_the_pinned_nightly() {
    let root = crate::support::fresh("plain-derives-expand");
    if std::env::var_os(CHILD).is_some() {
        expand_in(&root);
        return;
    }
    copy(Path::new(DERIVES), &root);
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "expand::a_crates_derive_expands_through_the_pinned_nightly",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env("RAWR_HOME", crate::support::runtime_home())
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn expand_in(root: &Path) {
    use rawr_import::expand::{directory, expand, Basis, Item};
    let target = rawr::project::Target::discover(root.join("src/main.rs")).unwrap();
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target).unwrap();
    let limits = graph.rust_limits();
    let environment = rawr_import::cargo::config::Environment::capture().unwrap();
    let request = graph
        .rust_inventory_plain(
            environment,
            &[],
            &limits,
            &mut rawr::package::Copies::default(),
        )
        .unwrap()
        .expect("the fixture has dependencies");
    let native = request.pinned_native(&limits).unwrap();
    let basis = Basis::capture(request, &native, &limits).unwrap();
    let item = Item {
        path: "Task".to_owned(),
        text: "#[derive(Debug, Clone, Serialize, Deserialize)]\nstruct Task {\n    \
               #[serde(rename = \"text\")]\n    description: String,\n    done: bool,\n}"
            .to_owned(),
        derives: vec![
            ("Serialize".to_owned(), "serde::Serialize".to_owned()),
            ("Deserialize".to_owned(), "serde::Deserialize".to_owned()),
        ],
    };
    // Another item, which writes its derive by another name: both expand
    // in one driver run, each with its own `use`.
    let filter = Item {
        path: "app::Filter".to_owned(),
        text: "#[derive(Debug, Save)]\nenum Filter { All, Active }".to_owned(),
        derives: vec![("Save".to_owned(), "serde::Serialize".to_owned())],
    };
    let kept = directory(root);
    let items = [item, filter];
    let expanded = expand(&items, &basis, "2024", &kept, &native, &limits).unwrap();
    assert_eq!(expanded.len(), 2);
    let derives = |index: usize| {
        expanded[index]
            .expansions
            .iter()
            .map(|expansion| expansion.derive.as_str())
            .collect::<Vec<_>>()
    };
    // std's `Debug` and `Clone` are not the crate's.
    assert_eq!(derives(0), ["Serialize", "Deserialize"]);
    assert_eq!(derives(1), ["Save"]);
    let text = |item: usize, index: usize| expanded[item].expansions[index].items.join("\n");
    assert!(
        text(0, 0).contains("impl _serde::Serialize for Task"),
        "{}",
        text(0, 0)
    );
    // The helper attribute reached the macro.
    assert!(text(0, 0).contains("\"text\""), "{}", text(0, 0));
    assert!(
        text(0, 1).contains("impl<'de> _serde::Deserialize<'de> for Task"),
        "{}",
        text(0, 1)
    );
    assert!(
        text(1, 0).contains("impl _serde::Serialize for Filter"),
        "{}",
        text(1, 0)
    );
    // Each expansion is kept, and the next one reads them with no compiler.
    let files = fs::read_dir(&kept)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        })
        .count();
    assert_eq!(files, 2);
    let again = expand(&items, &basis, "2024", &kept, &native, &limits).unwrap();
    assert_eq!(again[0].expansions.len(), 2);
    assert_eq!(again[1].expansions.len(), 1);
}
