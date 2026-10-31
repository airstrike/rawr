use std::path::{Path, PathBuf};

use crate::applicability::{inventory::Acquired, Catalog};
use crate::cargo::config::{Dependency, Environment};
use crate::cargo::{Limits, Request};
use crate::extract::Policy;
use crate::metrics::{recorder::Recorder, ProcessKind};
use crate::process::{test_launch, Native};

#[test]
fn native_equivalence_retains_transitive_sources_config_and_script_outputs() {
    let _turn = crate::store::test_turn();
    crate::store::isolate_for_tests();
    let limits = Limits::standard();
    let native = Native::pinned().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../target/tmp/native-equivalence-{}",
        std::process::id()
    ));
    let fixtures = root.join("crates");
    let original = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in ["a", "b", "c"] {
        let name = format!("applicability-{name}");
        let destination = fixtures.join(&name);
        std::fs::create_dir_all(destination.join("src")).unwrap();
        for file in ["Cargo.toml", "src/lib.rs"] {
            std::fs::copy(original.join(&name).join(file), destination.join(file)).unwrap();
        }
    }
    let b = fixtures.join("applicability-b");
    std::fs::write(b.join("declared.txt"), "unchanged declared input").unwrap();
    std::fs::write(b.join("build.rs"), r#"
fn main() {
    println!("cargo:rerun-if-changed=declared.txt");
    let value = std::env::var("FACTS_WITNESS_OUTPUT").unwrap_or_else(|_| "first".to_owned());
    println!("cargo:rustc-env=FACTS_GENERATED={value}");
    std::fs::write(std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("observed.txt"), value).unwrap();
}
"#).unwrap();
    let request = |name: &str, selected: &[&str], inventory, output| {
        request(
            &root.join(name),
            &fixtures,
            selected,
            inventory,
            output,
            &limits,
        )
    };
    let inventories = [
        Acquired::capture(request("ab", &["a", "b"], true, "first"), &native, &limits).unwrap(),
        Acquired::capture(request("cb", &["c", "b"], true, "first"), &native, &limits).unwrap(),
    ];
    let catalog =
        Catalog::join(&inventories).expect("B witnesses agree across independent harnesses");
    let capture = |request| {
        let recorder = Recorder::transform(1000).unwrap();
        let inputs = {
            let _scope = recorder.scope().unwrap();
            super::Inputs::capture(request, &native, &limits).unwrap()
        };
        let evidence = recorder.snapshot().unwrap();
        assert!(
            evidence
                .processes
                .iter()
                .all(|row| row.kind != ProcessKind::MetadataProbe || row.count == 0),
            "a consuming normal build must not extract combined API facts"
        );
        inputs
    };
    let baseline = capture(request("abc", &["a", "b", "c"], false, "first"));
    let no_children = || {
        test_launch::Script::new(
            std::fs::canonicalize(&root).unwrap(),
            test_launch::FileLimits { count: 0, bytes: 0 },
            Vec::new(),
        )
        .unwrap()
    };
    no_children()
        .run(|| {
            catalog
                .bind_native(baseline)
                .expect("same closure inputs bind without a child")
        })
        .unwrap();

    // This undeclared ambient value changes only observed script outputs, not
    // source, declarations, configuration or the compiler environment domain.
    let outputs = capture(request("changed-output", &["a", "b", "c"], false, "second"));
    assert!(no_children()
        .run(|| catalog.bind_native(outputs))
        .unwrap()
        .is_err());

    let config_root = root.join("changed-config");
    std::fs::create_dir_all(config_root.join(".cargo")).unwrap();
    std::fs::write(
        config_root.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"native_equivalence_changed\"]\n",
    )
    .unwrap();
    let configured = capture(request("changed-config", &["a", "b", "c"], false, "first"));
    assert!(no_children()
        .run(|| catalog.bind_native(configured))
        .unwrap()
        .is_err());

    let source = std::fs::read_to_string(b.join("src/lib.rs")).unwrap();
    std::fs::write(
        b.join("src/lib.rs"),
        format!("{source}\npub const CHANGED_INPUT: i64 = 17;\n"),
    )
    .unwrap();
    let changed = capture(request("changed-source", &["a", "b", "c"], false, "first"));
    assert!(no_children()
        .run(|| catalog.bind_native(changed))
        .unwrap()
        .is_err());
    eprintln!("INDEPENDENT_NATIVE_EQUIVALENCE captures=2 consumer_builds=4 metadata_probes=0 composition_children=0 mutations=source,config,script-output");
}

fn request(
    root: &Path,
    fixtures: &Path,
    selected: &[&str],
    inventory: bool,
    output: &str,
    limits: &Limits,
) -> Request {
    std::fs::create_dir_all(root).unwrap();
    let root = root.canonicalize().unwrap();
    let environment = Environment::capture()
        .unwrap()
        .setting("CARGO_NET_OFFLINE", "true")
        .unwrap()
        .setting("FACTS_WITNESS_OUTPUT", output)
        .unwrap();
    let mut builder = Request::builder(&root, &root.join("target/rawr/cache"), limits)
        .unwrap()
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .unwrap();
    if inventory {
        builder = builder.inventory();
    }
    for name in selected {
        let path: PathBuf = fixtures.join(format!("applicability-{name}"));
        builder = builder
            .dependency(
                Dependency::path(&format!("facts_{name}"), &path, limits)
                    .unwrap()
                    .package(&format!("facts-{name}"))
                    .unwrap(),
            )
            .unwrap();
    }
    builder.build().unwrap()
}
