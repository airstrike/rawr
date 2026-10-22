use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest as _, Sha256};

use super::{file, limited_file, optional_file, packages, scalar};
use crate::cargo::{unit, Limits};
use crate::input::proof::{Entry, Role};
use crate::input::stored::observe::slots::Slots;
use crate::input::stored::{Budget, Error, Recipe};
use crate::input::{build, scalar::Value, Allocation, Usage};
use crate::node;

const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rawr-input-prefix-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).expect("fixture directory");
        Self(fs::canonicalize(path).expect("canonical fixture directory"))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fact() -> Recipe {
    Recipe::Fact {
        role: Role::Target,
        package_id: None,
        name: "target".into(),
        value: "host".into(),
    }
}

fn value() -> Value<'static> {
    Value::Fact {
        role: Role::Target,
        package_id: None,
        name: "target",
        value: "host",
    }
}

#[test]
fn scalar_requires_the_next_recipe_without_skipping_or_owning_mismatches() {
    for mode in 0..5 {
        let mut recipes = vec![fact()];
        match mode {
            0 => recipes.clear(),
            1 => recipes.insert(
                0,
                Recipe::File {
                    role: Role::Authority,
                    package_id: None,
                    path: "/absent".into(),
                },
            ),
            2 => {
                if let Recipe::Fact { value, .. } = &mut recipes[0] {
                    *value = "other".into();
                }
            }
            3 => {
                if let Recipe::Fact { package_id, .. } = &mut recipes[0] {
                    *package_id = Some("other".into());
                }
            }
            4 => {
                recipes[0] = Recipe::EnvironmentProjection {
                    schema: 1,
                    key: "target".into(),
                    value: Some("host".into()),
                }
            }
            _ => unreachable!(),
        }
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all native work spent");
        let mut entries = Slots::new();
        let mut cursor = recipes.iter();
        let result = scalar(
            &mut cursor,
            &mut entries,
            value(),
            &mut Budget::borrow(&limits, &mut allocation, &graph),
        );
        assert!(matches!(result, Err(Error::Encoding(_))));
        assert!(entries.as_slice().is_empty());
        assert_eq!(allocation.usage(), PRIOR);
        if mode == 1 {
            assert!(matches!(cursor.next(), Some(Recipe::Fact { .. })));
        }
    }
}

#[test]
fn scalar_storage_uses_original_exact_floors_without_graph_decode_and_latches() {
    let bytes = std::mem::size_of::<Entry>() + "target".len() + "host".len();
    // Scalar entry storage is owned once by the original allocation.
    for input_floor in [0, 1] {
        let mut limits = Limits::standard();
        limits.input_proof_bytes = PRIOR.bytes + bytes - input_floor;
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
            .expect("all decoding already spent");
        graph
            .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
            .expect("all hash work spent");
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all native work spent");
        let recipes = [fact(), fact()];
        let mut cursor = recipes.iter();
        let mut entries = Slots::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = scalar(&mut cursor, &mut entries, value(), &mut budget);
        // The scalar storage is owned in full either way; a ceiling one
        // under records the byte axis with the exact total as its high water.
        result.expect("scalar storage is owned, not gated");
        assert_eq!(
            graph.census_usage().expect("graph census").0,
            node::MAX_DECODED_BYTES_PER_GRAPH
        );
        assert_eq!(entries.as_slice().len(), 1);
        assert!(
            matches!(&entries.as_slice()[0], Entry::Fact { name, value, .. }
            if name.as_ref() == "target" && value.as_ref() == "host")
        );
        assert_eq!(
            budget.usage(),
            Usage {
                items: PRIOR.items + 3,
                bytes: PRIOR.bytes + bytes
            }
        );
        let record = budget.exceedances();
        if input_floor == 0 {
            assert!(!record.is_exceeded(), "exact ownership");
        } else {
            let axis = record
                .axis(crate::input::INPUT_BYTES_RESOURCE)
                .expect("one-under byte axis recorded");
            assert_eq!(axis.target as usize, PRIOR.bytes + bytes - 1);
            assert_eq!(axis.high_water as usize, PRIOR.bytes + bytes);
            assert!(budget.check().is_ok(), "an exceedance never latches");
            // A genuine failure latches; a latched account consumes no recipe.
            budget.refuse();
            let remaining = cursor.len();
            assert!(matches!(
                scalar(&mut cursor, &mut entries, value(), &mut budget),
                Err(Error::Refused)
            ));
            assert_eq!(cursor.len(), remaining);
            assert_eq!(entries.as_slice().len(), 1);
        }
    }
    // The entry slot is the first charge: with a ceiling one byte under the
    // slot, its reservation is the first exceeding charge, before any text.
    let mut limits = Limits::standard();
    limits.input_proof_bytes = PRIOR.bytes + std::mem::size_of::<Entry>() - 1;
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
    scalar(
        &mut [fact()].iter(),
        &mut Slots::new(),
        value(),
        &mut budget,
    )
    .expect("the entry slot is owned, not gated");
    let record = budget.exceedances();
    let axis = record
        .axis(crate::input::INPUT_BYTES_RESOURCE)
        .expect("slot byte axis recorded");
    assert_eq!(
        axis.target as usize,
        PRIOR.bytes + std::mem::size_of::<Entry>() - 1
    );
    assert_eq!(
        axis.first_requested as usize,
        PRIOR.bytes + std::mem::size_of::<Entry>(),
        "the slot reservation precedes every scalar text charge"
    );
    assert_eq!(axis.high_water as usize, PRIOR.bytes + bytes);
    assert_eq!(budget.usage().bytes, PRIOR.bytes + bytes);
}

#[test]
fn native_path_role_and_package_mismatches_stop_before_filesystem_work() {
    let source = Path::new("/prefix/Cargo.toml");
    for mode in 0..4 {
        let recipe = Recipe::File {
            role: if mode == 0 {
                Role::Config
            } else {
                Role::Harness
            },
            package_id: if mode == 1 {
                Some("other".into())
            } else {
                None
            },
            path: if mode == 2 {
                "/prefix/./Cargo.toml".into()
            } else {
                source.into()
            },
        };
        let recipe = if mode == 3 {
            Recipe::Path {
                role: Role::Harness,
                package_id: None,
                path: source.into(),
            }
        } else {
            recipe
        };
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        graph
            .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
            .expect("all native work spent");
        let mut entries = Slots::new();
        assert!(matches!(
            file(
                &mut [recipe].iter(),
                &mut entries,
                Role::Harness,
                None,
                source,
                Path::new("/"),
                &mut Budget::borrow(&limits, &mut allocation, &graph)
            ),
            Err(Error::Encoding(_))
        ));
        assert_eq!(allocation.usage(), PRIOR);
        assert!(entries.as_slice().is_empty());
    }
}

#[test]
fn authority_then_optional_config_preserves_presence_digest_and_handoff() {
    let fixture = Fixture::new();
    let source = fixture.0.join("Cargo.toml");
    let absent = fixture.0.join("absent.toml");
    fs::write(&source, b"data").expect("authority file");
    let digest: [u8; 32] = Sha256::digest(b"data").into();
    for mode in 0..4 {
        let recipes = [
            Recipe::File {
                role: Role::Authority,
                package_id: None,
                path: source.clone(),
            },
            Recipe::Path {
                role: Role::Config,
                package_id: None,
                path: absent.clone(),
            },
            fact(),
        ];
        let limits = Limits::standard();
        let mut allocation = Allocation::for_proof(PRIOR, &limits);
        let graph = node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let mut entries = Slots::new();
        let mut cursor = recipes.iter();
        let result = limited_file(
            &mut cursor,
            &mut entries,
            Role::Authority,
            &source,
            if mode == 1 { &[0; 32] } else { &digest },
            &fixture.0,
            "authority",
            if mode == 2 { 3 } else { 4 },
            &mut budget,
        );
        if mode == 1 {
            assert!(result.is_err());
            assert!(entries.as_slice().is_empty());
            assert_eq!(cursor.len(), 2);
            continue;
        }
        result.expect("the supplied authority digest matches");
        // The authority's byte bound is an accounting target; a file one
        // byte over it is observed, digested and recorded on the bound's axis.
        let record = budget.exceedances();
        let axis = record.axis("authority");
        if mode == 2 {
            let axis = axis.expect("authority bound axis recorded");
            assert_eq!((axis.target, axis.high_water), (3, 4));
            assert!(budget.check().is_ok(), "an exceedance never latches");
        } else {
            assert!(axis.is_none());
        }
        let result = optional_file(
            &mut cursor,
            &mut entries,
            &absent,
            if mode == 3 { Some(&digest) } else { None },
            &fixture.0,
            4,
            &mut budget,
        );
        if mode == 3 {
            assert!(matches!(result, Err(Error::Encoding(_))));
            assert_eq!(entries.as_slice().len(), 1);
        } else {
            result.expect("the supplied config absence matches");
            assert!(matches!(
                &entries.as_slice()[0],
                Entry::File {
                    role: Role::Authority,
                    ..
                }
            ));
            assert!(matches!(
                &entries.as_slice()[1],
                Entry::Missing {
                    role: Role::Config,
                    ..
                }
            ));
        }
        assert!(matches!(cursor.next(), Some(Recipe::Fact { .. })));
    }
}

#[test]
fn package_inventory_interleaves_files_and_scalars_and_preserves_the_next_recipe() {
    let fixture = Fixture::new();
    let mut inventory = BTreeMap::new();
    let mut current = Vec::new();
    let mut recipes = Vec::new();
    for id in ["a", "z"] {
        let root = fixture.0.join(id);
        fs::create_dir(&root).expect("package directory");
        let manifest = root.join("Cargo.toml");
        fs::write(&manifest, b"[package]\n").expect("manifest");
        let script = if id == "a" {
            let path = root.join("build.rs");
            fs::write(&path, b"fn main() {}\n").expect("build script");
            Some(path)
        } else {
            None
        };
        recipes.push(Recipe::File {
            role: Role::PackageManifest,
            package_id: Some(id.into()),
            path: manifest.clone(),
        });
        if let Some(path) = &script {
            recipes.push(Recipe::File {
                role: Role::BuildScript,
                package_id: Some(id.into()),
                path: path.clone(),
            });
        }
        recipes.push(Recipe::Fact {
            role: Role::DependencySource,
            package_id: Some(id.into()),
            name: format!("package:{id}").into(),
            value: root.to_str().expect("fixture text").into(),
        });
        inventory.insert(
            id.into(),
            build::Package {
                manifest,
                root: root.clone(),
                build_script: script,
            },
        );
        current.push(unit::Package::fixture(
            id,
            id,
            "1.0.0",
            root,
            unit::PackageSource::Path {
                cargo_id: id.into(),
            },
        ));
    }
    recipes.push(fact());
    let build = build::Retained {
        build: build::Build {
            evidence: build::Evidence {
                usage: PRIOR,
                packages: inventory.into(),
                scripts: Box::default(),
                artifacts: Box::default(),
                environment: Box::default(),
            },
            package_files: BTreeMap::new(),
        },
    };
    let metadata = unit::Metadata::fixture_packages(current);
    let limits = Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let mut entries = Slots::new();
    let mut cursor = recipes.iter();
    packages(
        &mut cursor,
        &mut entries,
        &build,
        &metadata,
        &fixture.0,
        &mut Budget::borrow(&limits, &mut allocation, &graph),
    )
    .expect("complete ordered package group");
    let roles: Vec<_> = entries
        .as_slice()
        .iter()
        .map(|entry| match entry {
            Entry::File { role, .. } | Entry::Fact { role, .. } => *role,
            _ => panic!("only files and package facts"),
        })
        .collect();
    assert_eq!(
        roles,
        [
            Role::PackageManifest,
            Role::BuildScript,
            Role::DependencySource,
            Role::PackageManifest,
            Role::DependencySource
        ]
    );
    assert!(matches!(
        cursor.next(),
        Some(Recipe::Fact {
            role: Role::Target,
            ..
        })
    ));
    let mut cursor = recipes.iter();
    let mut allocation = Allocation::for_proof(PRIOR, &limits);
    let graph = node::GraphBudget::new();
    graph
        .paths(node::MAX_PATH_CHECKS_PER_GRAPH)
        .expect("no filesystem work remains");
    assert!(matches!(
        packages(
            &mut cursor,
            &mut Slots::new(),
            &build,
            &unit::Metadata::fixture_packages([]),
            &fixture.0,
            &mut Budget::borrow(&limits, &mut allocation, &graph)
        ),
        Err(Error::Encoding(_))
    ));
    assert_eq!(cursor.len(), recipes.len());
    assert_eq!(allocation.usage(), PRIOR);
}
