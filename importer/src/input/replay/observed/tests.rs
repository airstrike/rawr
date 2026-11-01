use std::collections::BTreeMap;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest as _, Sha256};

use super::saved_output;
use crate::cargo;
use crate::input::build;
use crate::input::declaration::{self, Declarations};
use crate::input::proof::Entry;
use crate::input::replay;
use crate::input::stored::{self, Budget};
use crate::input::{Allocation, Usage};
use crate::node;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

mod dependency;

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &cargo::Limits::standard())
}

fn data() -> Declarations {
    Declarations {
        package_id: "pkg".into(),
        output: "/saved-output/output".into(),
        directives: Box::from([Box::from("opaque=\"é\\\r\0\t")]),
        paths: Box::from([Box::from("z"), Box::from("a"), Box::from("z")]),
        environment: Box::from([Box::from("K"), Box::from("K")]),
    }
}

fn serialized(declarations: &Declarations) -> Vec<u8> {
    serde_json::to_vec(&replay::SavedOutput::borrow(declarations)).expect("reference record")
}

fn ownership(declarations: &Declarations, execution: &Path) -> Usage {
    Usage {
        items: 5,
        bytes: declarations.package_id.len()
            + execution.as_os_str().len()
            + "saved-output".len()
            + declarations.output.as_os_str().len()
            + 64,
    }
}

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-structured-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join("build/package-hash/out")).expect("fixture layout");
        Self(fs::canonicalize(root).expect("canonical fixture"))
    }

    fn out(&self) -> PathBuf {
        self.0.join("build/package-hash/out")
    }

    fn output(&self) -> PathBuf {
        self.0.join("build/package-hash/output")
    }

    fn build(&self) -> build::Retained {
        build::Retained {
            build: build::Build {
                evidence: build::Evidence {
                    usage: Usage {
                        items: 100,
                        bytes: 1000,
                    },
                    packages: BTreeMap::from([(
                        "pkg".into(),
                        build::Package {
                            manifest: self.0.join("Cargo.toml"),
                            root: self.0.clone(),
                            build_script: None,
                        },
                    )])
                    .into(),
                    scripts: Box::from([build::Script {
                        compiler: None,
                        package_id: "pkg".into(),
                        out_dir: self.out(),
                        cfg: Box::from([]),
                        environment: Box::from([]),
                    }]),
                    artifacts: Box::from([]),
                    environment: Box::from([]),
                },
                package_files: BTreeMap::new(),
            },
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn acquired_data_matches_legacy_after_source_removal_and_preserves_execution_spelling() {
    let fixture = Fixture::new();
    let build = fixture.build();
    for bytes in [
        &b""[..],
        &b"ignored\ncargo:rerun-if-changed=z\ncargo::rerun-if-env-changed=K\ncargo:rerun-if-changed=z\ncargo:opaque=\"quoted\"\\value\r\n"[..],
    ] {
        fs::write(fixture.output(), bytes).expect("saved output");
        let graph = node::GraphBudget::new();
        let mut allocation = account();
        let declarations = declaration::observed::read(
            "pkg", &fixture.out(), &build, &fixture.0,
            &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        ).expect("associated current acquisition");
        let execution = fixture.out().join(".");
        let expected = replay::saved_output_entry(
            "pkg",
            &execution,
            &mut account(),
        ).expect("legacy full entry");
        let pointers = (declarations.directives.as_ptr(), declarations.paths.as_ptr(), declarations.environment.as_ptr());
        fs::remove_file(fixture.output()).expect("source removed after parsing");
        graph.exhaust_paths();
        let before = allocation.usage();
        let actual = saved_output(
            &declarations, &execution,
            &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
        ).expect("formatting has no filesystem work");
        assert!(actual == expected, "current and legacy entries differ");
        let Entry::BuildMessage { execution: retained, .. } = actual else { panic!("BuildMessage") };
        assert_eq!(retained.as_ref(), execution.to_str().expect("fixture spelling"));
        assert_ne!(retained.as_ref(), fixture.out().to_str().expect("canonical spelling"));
        assert_eq!(pointers, (declarations.directives.as_ptr(), declarations.paths.as_ptr(), declarations.environment.as_ptr()));
        let added = ownership(&declarations, &execution);
        assert_eq!(allocation.usage(), Usage { items: before.items + added.items, bytes: before.bytes + added.bytes });
    }
}

#[test]
fn shared_schema_preserves_field_order_duplicates_and_exact_json_escaping() {
    let declarations = data();
    let expected = r#"{"directives":["opaque=\"é\\\r\u0000\t"],"paths":["z","a","z"],"environment":["K","K"]}"#;
    assert_eq!(serialized(&declarations), expected.as_bytes());
    let actual = saved_output(
        &declarations,
        Path::new("relative/./execution"),
        &mut Budget::borrow(
            &cargo::Limits::standard(),
            &mut account(),
            &node::GraphBudget::new(),
        ),
    )
    .expect("structured record");
    let Entry::BuildMessage {
        package_id,
        execution,
        name,
        source,
        value,
    } = actual
    else {
        panic!("BuildMessage")
    };
    assert_eq!(package_id.as_ref(), "pkg");
    assert_eq!(execution.as_ref(), "relative/./execution");
    assert_eq!(name.as_ref(), "saved-output");
    assert_eq!(source.as_deref(), Some("/saved-output/output"));
    assert_eq!(
        value.as_ref(),
        format!("{:x}", Sha256::digest(expected.as_bytes()))
    );
}

#[test]
fn original_remaining_account_is_exact_and_refusal_survives_a_new_graph_argument() {
    let declarations = data();
    let execution = Path::new("execution");
    let added = ownership(&declarations, execution);
    let mut allocation = account();
    let result = saved_output(
        &declarations,
        execution,
        &mut Budget::borrow(
            &cargo::Limits::standard(),
            &mut allocation,
            &node::GraphBudget::new(),
        ),
    );
    assert!(result.is_ok());
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + added.items,
            bytes: PRIOR.bytes + added.bytes
        }
    );
    // A genuine failure latches the original account; the latch
    // survives a new graph argument and no further work is charged.
    allocation.refuse();
    let before = allocation.usage();
    assert!(matches!(
        saved_output(
            &declarations,
            execution,
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            ),
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), before);
    assert!(matches!(
        allocation.items(0),
        Err(crate::Error::InputAccountRefused)
    ));
}

#[test]
fn graph_hash_is_exact_and_retained_entry_storage_adds_no_decode() {
    let declarations = data();
    let execution = Path::new("execution");
    let length = serialized(&declarations).len();
    let added = ownership(&declarations, execution);
    // Retained entries are copies owned once by the original allocation.
    for kind in 0..2 {
        {
            let graph = node::GraphBudget::new();
            graph
                .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
                .expect("all paths already spent");
            graph.exhaust_comparisons();
            if kind == 0 {
                graph
                    .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize - length)
                    .expect("prior hash work");
            } else {
                graph
                    .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                    .expect("all decoding already spent");
            }
            let mut allocation = account();
            let result = saved_output(
                &declarations,
                execution,
                &mut Budget::borrow(&cargo::Limits::standard(), &mut allocation, &graph),
            );
            assert!(result.is_ok());
            assert_eq!(
                allocation.usage(),
                Usage {
                    items: PRIOR.items + added.items,
                    bytes: PRIOR.bytes + added.bytes
                }
            );
            if kind == 0 {
                // The hash charge is exactly the serialized length.
                assert_eq!(
                    graph.census_usage().expect("graph census").2,
                    node::MAX_HASHED_BYTES_PER_GRAPH
                );
            } else {
                assert_eq!(
                    graph.census_usage().expect("graph census").0,
                    node::MAX_DECODED_BYTES_PER_GRAPH
                );
            }
        }
    }
}

#[test]
fn path_and_digest_values_do_not_acquire_scalar_limits() {
    let declarations = data();
    let mut limits = cargo::Limits::standard();
    limits.string_bytes = "saved-output".len();
    let execution = Path::new("longer-than-scalar/./execution");
    let mut exact = account();
    let result = saved_output(
        &declarations,
        execution,
        &mut Budget::borrow(&limits, &mut exact, &node::GraphBudget::new()),
    )
    .expect("path/value fields retain non-scalar semantics");
    let Entry::BuildMessage { value, .. } = result else {
        panic!("BuildMessage")
    };
    assert_eq!(value.len(), 64);
    // One byte under the scalar target records the string axis with the
    // exact scalar length ("saved-output"), not the path or digest length.
    limits.string_bytes -= 1;
    let mut under = account();
    saved_output(
        &declarations,
        execution,
        &mut Budget::borrow(&limits, &mut under, &node::GraphBudget::new()),
    )
    .expect("admitted");

    let invalid = PathBuf::from(std::ffi::OsString::from_vec(b"execution\xff".to_vec()));
    assert!(matches!(
        saved_output(
            &declarations,
            &invalid,
            &mut Budget::borrow(
                &cargo::Limits::standard(),
                &mut account(),
                &node::GraphBudget::new()
            ),
        ),
        Err(stored::Error::Encoding(
            "structured replay path is not Unicode"
        ))
    ));
}
