use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use crate::cargo::Limits;
use crate::input::stored::{self, Budget};
use crate::input::{build, declaration, proof, script, Allocation, Usage};
use crate::{node, Error, Result};

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

struct Fixture {
    script: build::Script,
    package: build::Package,
    declarations: declaration::Declarations,
    environment: Vec<(OsString, OsString)>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            script: super::script(),
            package: build::Package {
                root: "/fixture/pkg".into(),
                manifest: "/fixture/pkg/Cargo.toml".into(),
                build_script: None,
            },
            declarations: declaration::Declarations {
                package_id: "pkg".into(),
                output: "/build/unit/output".into(),
                directives: Box::from([Box::from("opaque=raw"), Box::from("rerun-if-changed=src")]),
                paths: Box::from([Box::from("src/../data/file"), Box::from("/external/./file")]),
                environment: Box::from([
                    Box::from("SET"),
                    Box::from("EMPTY"),
                    Box::from("UNSET"),
                    Box::from("SET"),
                ]),
            },
            environment: vec![
                ("SET".into(), "from-build".into()),
                ("EMPTY".into(), "".into()),
            ],
        }
    }

    fn collect(&self, allocation: &mut Allocation) -> Result<Vec<proof::Recipe>> {
        let mut recipes = Vec::new();
        let execution = script::declarations(
            &self.script,
            &self.package,
            &self.declarations,
            false,
            &self.environment,
            allocation,
            |value, allocation| {
                allocation.items(1)?;
                recipes.try_reserve(1).expect("fixture recipe storage");
                recipes.push(value.retain(allocation)?);
                Ok(())
            },
        )?;
        assert_eq!(
            execution.as_ptr(),
            self.script
                .out_dir
                .to_str()
                .expect("fixture execution")
                .as_ptr()
        );
        Ok(recipes)
    }

    fn legacy(&self, allocation: &mut Allocation) -> Result<Vec<proof::Recipe>> {
        let mut recipes = Vec::new();
        macro_rules! recipe {
            ($value:expr) => {{
                allocation.items(1)?;
                recipes.try_reserve(1).expect("fixture recipe storage");
                recipes.push($value);
            }};
        }
        let package_id = &self.script.package_id;
        allocation.path(&self.script.out_dir)?;
        let execution =
            crate::input::path_text("build-script execution OUT_DIR", &self.script.out_dir)?;
        recipe!(proof::Recipe::saved_output(
            package_id,
            &self.script.out_dir,
            allocation
        )?);
        for (index, directive) in self.declarations.directives.iter().enumerate() {
            recipe!(proof::Recipe::directive(
                package_id, execution, index, directive, allocation
            )?);
        }
        for declared in &self.declarations.paths {
            let path = crate::input::declared_path(&self.package.root, declared)?;
            recipe!(proof::Recipe::package_path(
                proof::Role::DeclaredInput,
                package_id,
                &path,
                allocation
            )?);
        }
        for key in &self.declarations.environment {
            recipe!(proof::Recipe::environment(
                package_id,
                execution,
                key,
                crate::input::environment_value(&self.environment, key),
                allocation
            )?);
        }
        Ok(recipes)
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &Limits::standard())
}

#[derive(Debug, Eq, PartialEq)]
enum Row {
    Saved(String, PathBuf),
    Directive(String, String, usize, String),
    Path(String, PathBuf),
    Environment(String, String, String, Option<String>),
}

fn rows(recipes: &[proof::Recipe]) -> Vec<Row> {
    recipes
        .iter()
        .map(|recipe| match recipe {
            proof::Recipe::SavedOutput {
                package_id,
                out_dir,
            } => Row::Saved(package_id.to_string(), out_dir.clone()),
            proof::Recipe::Directive {
                package_id,
                execution,
                index,
                value,
            } => Row::Directive(
                package_id.to_string(),
                execution.to_string(),
                *index,
                value.to_string(),
            ),
            proof::Recipe::Path {
                role: proof::Role::DeclaredInput,
                package_id,
                path,
            } => Row::Path(
                package_id.as_deref().expect("package owner").to_string(),
                path.clone(),
            ),
            proof::Recipe::Environment {
                package_id,
                execution,
                key,
                value,
            } => Row::Environment(
                package_id.to_string(),
                execution.to_string(),
                key.to_string(),
                value.as_deref().map(str::to_string),
            ),
            _ => panic!("only declaration recipes"),
        })
        .collect()
}

#[test]
fn cold_emission_matches_original_records_order_and_every_item_floor() {
    let fixture = Fixture::new();
    let mut reference = account();
    let expected = fixture
        .legacy(&mut reference)
        .expect("original declarations");
    let exact = reference.usage();
    for item_limit in PRIOR.items..=exact.items {
        let mut actual = account();
        actual.item_limit = item_limit;
        let mut original = account();
        original.item_limit = item_limit;
        let actual_result = fixture.collect(&mut actual);
        let original_result = fixture.legacy(&mut original);
        assert_eq!(
            actual_result.as_ref().err().map(ToString::to_string),
            original_result.as_ref().err().map(ToString::to_string)
        );
        assert_eq!(actual.usage(), original.usage());
        if let Ok(recipes) = actual_result {
            assert_eq!(rows(&recipes), rows(&expected));
        }
    }
    for byte_limit in [exact.bytes - 1, exact.bytes] {
        let mut actual = account();
        actual.byte_limit = byte_limit;
        let mut original = account();
        original.byte_limit = byte_limit;
        assert_eq!(
            fixture.collect(&mut actual).is_ok(),
            fixture.legacy(&mut original).is_ok()
        );
        assert_eq!(actual.usage(), original.usage());
    }
}

#[test]
fn current_paths_use_exact_original_storage_without_graph_decode_or_filesystem_work() {
    let fixture = Fixture::new();
    let bytes = fixture
        .declarations
        .paths
        .iter()
        .map(|value| {
            let path = Path::new(value.as_ref());
            2 * if path.is_absolute() {
                path.as_os_str().len()
            } else {
                fixture.package.root.as_os_str().len() + 1 + path.as_os_str().len()
            }
        })
        .sum::<usize>();
    // Declared path copies are owned once by the original allocation.
    for input_floor in [0, 1] {
        let mut allocation = account();
        allocation.byte_limit = PRIOR.bytes + bytes - input_floor;
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all path work already spent");
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .expect("all hashing already spent");
        let mut paths = Vec::new();
        let result = script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            false,
            &fixture.environment,
            &mut Budget::borrow(&Limits::standard(), &mut allocation, &graph),
            |value, _| {
                if let script::Value::DeclaredPath { path, .. } = value {
                    paths.push(path.to_path_buf());
                }
                Ok(())
            },
        );
        // The exact remainder records nothing; one less byte records the
        // exact total as an exceedance and the declarations still emit.
        result.expect("accounting targets never refuse");
        assert_eq!(
            paths,
            [
                PathBuf::from("/fixture/pkg/data/file"),
                PathBuf::from("/external/file")
            ]
        );
        assert_eq!(
            allocation.usage(),
            Usage {
                items: PRIOR.items + 4,
                bytes: PRIOR.bytes + bytes
            }
        );
        let record = allocation.exceedances();
        if input_floor == 0 {
            assert!(!record.is_exceeded());
        } else {
            let axis = record
                .axis(crate::input::INPUT_BYTES_RESOURCE)
                .expect("one less byte records the byte axis");
            assert_eq!(axis.target, (PRIOR.bytes + bytes - 1) as u64);
            assert_eq!(axis.high_water, (PRIOR.bytes + bytes) as u64);
            assert_eq!(record.axes().count(), 1);
        }
        // Every graph axis was charged exactly to its target beforehand, so any
        // decode, path or hash work here would have recorded an exceedance.
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH
        );
        assert!(
            !graph.exceedances().is_exceeded(),
            "declared paths do no graph decode, path or hash work"
        );
        allocation
            .items(0)
            .expect("an exceedance never latches the account");
    }
    // Only a genuine failure latches; the latch precedes every emission.
    let mut allocation = account();
    allocation.refuse();
    let spent = allocation.usage();
    assert!(matches!(
        script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            false,
            &fixture.environment,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            ),
            |_, _| panic!("refusal must precede emissions")
        ),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), spent);
}

#[test]
fn execution_charge_and_encoding_precede_override_rejection() {
    let mut fixture = Fixture::new();
    let mut allocation = account();
    assert!(matches!(
        script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            true,
            &fixture.environment,
            &mut allocation,
            |_, _| panic!("overrides reject before first callback")
        ),
        Err(Error::Input {
            code: "cargo-env-config-unproved",
            ..
        })
    ));
    assert_eq!(
        allocation.usage(),
        Usage {
            items: PRIOR.items + 1,
            bytes: PRIOR.bytes + fixture.script.out_dir.as_os_str().len()
        }
    );
    let mut current = account();
    assert!(matches!(
        script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            true,
            &fixture.environment,
            &mut Budget::borrow(&Limits::standard(), &mut current, &node::GraphBudget::new()),
            |_, _| panic!("overrides reject before first callback")
        ),
        Err(stored::Error::Encoding("cargo-env-config-unproved"))
    ));
    assert_eq!(current.usage(), PRIOR);
    current.items(0).expect("override failure is semantic");
    fixture.script.out_dir = PathBuf::from(OsString::from_vec(b"/out/\xff".to_vec()));
    let mut allocation = account();
    assert!(matches!(
        script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            true,
            &fixture.environment,
            &mut allocation,
            |_, _| panic!("encoding rejects first")
        ),
        Err(Error::Path { .. })
    ));
    assert_eq!(allocation.usage().items, PRIOR.items + 1);
}

#[test]
fn callback_failure_prevents_later_path_normalization() {
    let mut fixture = Fixture::new();
    fixture.declarations.paths =
        Box::from(["x".repeat(crate::input::LINE_BYTES + 1).into_boxed_str()]);
    let graph = node::GraphBudget::new();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .expect("all storage already spent");
    let mut allocation = account();
    let mut calls = 0;
    let result = script::declarations(
        &fixture.script,
        &fixture.package,
        &fixture.declarations,
        false,
        &fixture.environment,
        &mut Budget::borrow(&Limits::standard(), &mut allocation, &graph),
        |value, _| {
            calls += 1;
            if matches!(value, script::Value::Directive { index: 1, .. }) {
                Err(stored::Error::Encoding("fixture-stop"))
            } else {
                Ok(())
            }
        },
    );
    assert!(matches!(
        result,
        Err(stored::Error::Encoding("fixture-stop"))
    ));
    assert_eq!(calls, 3);
    assert_eq!(allocation.usage(), PRIOR);
}

#[test]
fn raw_path_bound_records_exceedance_before_storage_and_root_escape_stays_semantic() {
    let mut fixture = Fixture::new();
    let raw = "x".repeat(crate::input::LINE_BYTES + 1);
    fixture.declarations.paths = Box::from([raw.clone().into_boxed_str()]);
    let mut allocation = account();
    let mut declared = Vec::new();
    // The raw path bound is an accounting target: the oversized spelling
    // is recorded before any storage of it, then normalized and stored on the
    // original account, and the declaration still emits.
    script::declarations(
        &fixture.script,
        &fixture.package,
        &fixture.declarations,
        false,
        &fixture.environment,
        &mut Budget::borrow(
            &Limits::standard(),
            &mut allocation,
            &node::GraphBudget::new(),
        ),
        |value, budget| {
            if let script::Value::DeclaredPath { path, .. } = value {
                assert!(budget.exceedances().axis("input path bytes").is_some());
                declared.push(path.to_path_buf());
            }
            Ok(())
        },
    )
    .expect("accounting targets never refuse");
    assert_eq!(declared, [fixture.package.root.join(&raw)]);
    let axis = allocation
        .exceedances()
        .axis("input path bytes")
        .copied()
        .expect("the raw path bound records its axis");
    assert_eq!(axis.target, crate::input::LINE_BYTES as u64);
    assert_eq!(axis.high_water, (crate::input::LINE_BYTES + 1) as u64);
    assert!(allocation.usage().items > PRIOR.items);
    assert!(
        allocation.usage().bytes > PRIOR.bytes + crate::input::LINE_BYTES,
        "the oversized path is stored on the original account after the record"
    );
    allocation
        .bytes(0)
        .expect("an exceedance never latches the account");
    fixture.declarations.paths = Box::from([Box::from("/../../escape")]);
    let mut allocation = account();
    assert!(matches!(
        script::declarations(
            &fixture.script,
            &fixture.package,
            &fixture.declarations,
            false,
            &fixture.environment,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            ),
            |_, _| Ok(())
        ),
        Err(stored::Error::Observation(stored::observe::Error::Policy(
            "path-escape"
        )))
    ));
    allocation
        .items(0)
        .expect("path escape is distinct from refusal");
}

#[test]
fn environment_lookup_reuses_retained_build_values_and_preserves_duplicate_declarations() {
    let mut fixture = Fixture::new();
    fixture.declarations.paths = Box::default();
    fixture
        .environment
        .insert(0, ("SET".into(), OsString::from_vec(vec![0xff])));
    let mut environment = Vec::new();
    script::declarations(
        &fixture.script,
        &fixture.package,
        &fixture.declarations,
        false,
        &fixture.environment,
        &mut Budget::borrow(
            &Limits::standard(),
            &mut account(),
            &node::GraphBudget::new(),
        ),
        |value, _| {
            if let script::Value::Environment { key, value, .. } = value {
                environment.push((key.to_string(), value.map(str::to_string)));
                if key == "SET" {
                    assert_eq!(
                        value.map(str::as_ptr),
                        fixture.environment[1].1.to_str().map(str::as_ptr)
                    );
                }
            }
            Ok(())
        },
    )
    .expect("retained environment borrows");
    assert_eq!(
        environment,
        [
            ("SET".into(), Some("from-build".into())),
            ("EMPTY".into(), Some("".into())),
            ("UNSET".into(), None),
            ("SET".into(), Some("from-build".into())),
        ]
    );
}

#[test]
fn fallback_ignores_opaque_directives_and_requires_both_rerun_lists_empty() {
    let mut fixture = Fixture::new();
    assert!(!script::fallback(&fixture.declarations));
    fixture.declarations.paths = Box::default();
    assert!(!script::fallback(&fixture.declarations));
    fixture.declarations.environment = Box::default();
    assert!(script::fallback(&fixture.declarations));
    assert!(!fixture.declarations.directives.is_empty());
    fixture.declarations.paths = Box::from([Box::from("src")]);
    assert!(!script::fallback(&fixture.declarations));
}
