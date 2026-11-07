use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest as _, Sha256};

use super::{acquire, registry};
use crate::cargo::{toolchain, unit, Limits};
use crate::input::package::{Set, Stored};
use crate::input::proof::{Entry, Role};
use crate::input::stored::{self, observe, Budget};
use crate::input::{Allocation, Usage};
use crate::node;

const SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const ID: &str = "registry+https://github.com/rust-lang/crates.io-index#crunchy@0.2.4";
const PRIOR: Usage = Usage {
    items: 7,
    bytes: 31,
};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
    package: unit::Package,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-registry-observed-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let path = root.join("cargo-home/registry/src/index-fixture/crunchy-0.2.4");
        fs::create_dir_all(&path).expect("fixture registry");
        let root = fs::canonicalize(root).expect("canonical fixture");
        let home = root.join("cargo-home");
        let path = fs::canonicalize(path).expect("canonical package");
        fs::write(
            path.join("Cargo.toml"),
            b"[package]\nname='crunchy'\nversion='0.2.4'\n",
        )
        .expect("manifest");
        fs::write(path.join(".cargo-ok"), b"{\"v\":1}").expect("completed marker");
        let package = unit::Package::fixture(
            ID,
            "crunchy",
            "0.2.4",
            path,
            unit::PackageSource::Registry {
                cargo_id: SOURCE.into(),
                checksum: Some([7; 32]),
            },
        );
        Self {
            root,
            home,
            package,
        }
    }

    fn capture(&self) -> Set {
        Set::capture_registry(
            toolchain::supported_cargo_commit(),
            &self.package,
            &self.home,
            PRIOR,
            &Limits::standard(),
        )
        .expect("actual legacy capture")
    }

    fn retained(&self) -> Stored {
        let Set::Registry(registry) = self.capture() else {
            panic!("registry capture")
        };
        registry.into_stored()
    }

    fn write(&self, bytes: &[u8]) {
        fs::write(self.package.root().join(".cargo-ok"), bytes).expect("marker update");
    }

    fn observe(
        &self,
        retained: &Stored,
        allocation: &mut Allocation,
        graph: &node::GraphBudget,
    ) -> stored::Result<Entry> {
        registry(
            retained,
            &self.package,
            &self.home,
            &self.root,
            &mut Budget::borrow(&Limits::standard(), allocation, graph),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn account() -> Allocation {
    Allocation::for_proof(PRIOR, &Limits::standard())
}

fn policy<T>(result: stored::Result<T>, expected: &str) {
    assert!(
        matches!(result, Err(stored::Error::Observation(observe::Error::Policy(actual)))
        if actual == expected),
        "expected policy {expected}"
    );
}

#[test]
fn actual_registry_entry_matches_legacy_without_historical_account_authority() {
    let fixture = Fixture::new();
    let captured = fixture.capture();
    let expected = captured
        .entry(&mut account(), &crate::cargo::Limits::standard())
        .expect("legacy entry");
    let Set::Registry(captured) = captured else {
        panic!("registry")
    };
    let mut retained = captured.into_stored();
    retained.base_usage = Usage {
        items: usize::MAX,
        bytes: usize::MAX,
    };
    retained.usage = retained.base_usage;
    let pointers = (
        retained.package_id.as_ptr(),
        retained.source.as_ptr(),
        retained.root.as_os_str().as_encoded_bytes().as_ptr(),
    );
    let mut allocation = account();
    let graph = node::GraphBudget::new();
    let actual = fixture
        .observe(&retained, &mut allocation, &graph)
        .expect("current original account");
    assert!(actual == expected, "registry entries differ");
    assert_eq!(
        pointers,
        (
            retained.package_id.as_ptr(),
            retained.source.as_ptr(),
            retained.root.as_os_str().as_encoded_bytes().as_ptr()
        )
    );
    assert!(allocation.usage().bytes > PRIOR.bytes);
    let Entry::Registry {
        role,
        package_id,
        source,
        checksum,
        root,
        marker_schema,
        marker_digest,
    } = actual
    else {
        panic!("Registry data")
    };
    assert_eq!(role, Role::DefaultPackageSource);
    assert_eq!(package_id.as_ref(), ID);
    assert_eq!(source.as_ref(), SOURCE);
    assert_eq!(checksum, [7; 32]);
    assert_eq!(
        root.as_ref(),
        fixture.package.root().to_str().expect("fixture path")
    );
    assert_eq!(marker_schema, 1);
    let expected_digest: [u8; 32] = Sha256::digest(b"{\"v\":1}").into();
    assert_eq!(marker_digest, expected_digest);
}

#[test]
fn exact_current_package_and_retained_marker_association_precede_storage_and_io() {
    let fixture = Fixture::new();
    for mismatch in 0..10 {
        let mut retained = fixture.retained();
        let mut current = fixture.package.clone();
        match mismatch {
            0 => retained.package_id = "other".into(),
            1 => retained.source = "registry+other".into(),
            2 => retained.checksum[0] ^= 1,
            3 => retained.root.push("."),
            4 => retained.marker = fixture.root.join("unrelated-marker"),
            5 => {
                current = unit::Package::fixture(
                    ID,
                    "different",
                    "0.2.4",
                    retained.root.clone(),
                    current.source().clone(),
                )
            }
            6 => {
                current = unit::Package::fixture(
                    ID,
                    "crunchy",
                    "9.9.9",
                    retained.root.clone(),
                    current.source().clone(),
                )
            }
            7 => {
                current = unit::Package::fixture(
                    ID,
                    "crunchy",
                    "0.2.4",
                    retained.root.clone(),
                    unit::PackageSource::Registry {
                        cargo_id: SOURCE.into(),
                        checksum: None,
                    },
                )
            }
            8 => {
                current = unit::Package::fixture(
                    ID,
                    "crunchy",
                    "0.2.4",
                    retained.root.clone(),
                    unit::PackageSource::Path {
                        cargo_id: "path+file:///elsewhere".into(),
                    },
                )
            }
            9 => retained.marker.push("."),
            _ => unreachable!(),
        }
        let mut allocation = account();
        let graph = node::GraphBudget::new();
        assert!(matches!(
            registry(
                &retained,
                &current,
                &fixture.home,
                &fixture.root,
                &mut Budget::borrow(&Limits::standard(), &mut allocation, &graph)
            ),
            Err(stored::Error::Observation(observe::Error::Policy(_)))
        ));
        assert_eq!(allocation.usage(), PRIOR);
        allocation
            .items(0)
            .expect("association mismatch is semantic");
    }
    assert!(!super::child(
        Path::new("/pkg"),
        "Cargo.toml",
        Path::new("/pkg/./Cargo.toml")
    ));
    assert!(!super::child(
        Path::new("/pkg"),
        "Cargo.toml",
        Path::new("/pkg/Cargo.toml/")
    ));
    assert!(super::child(
        Path::new("/"),
        "Cargo.toml",
        Path::new("/Cargo.toml")
    ));
}

#[test]
fn canonical_cargo_home_containment_and_derived_marker_policy_are_enforced() {
    let fixture = Fixture::new();
    let retained = fixture.retained();
    let other_home = fixture.root.join("other-home");
    fs::create_dir_all(other_home.join("registry/src")).expect("other source root");
    policy(
        registry(
            &retained,
            &fixture.package,
            &other_home,
            &fixture.root,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut account(),
                &node::GraphBudget::new(),
            ),
        ),
        "registry-root-outside-cargo-home",
    );
    let wrong_root = fixture.home.join("registry/src/index-fixture/other-0.2.4");
    fs::create_dir_all(&wrong_root).expect("mismatched package directory spelling");
    let wrong_package = unit::Package::fixture(
        ID,
        "crunchy",
        "0.2.4",
        wrong_root.clone(),
        fixture.package.source().clone(),
    );
    let mut wrong_retained = fixture.retained();
    wrong_retained.root = wrong_root;
    wrong_retained.marker = wrong_retained.root.join(".cargo-ok");
    policy(
        registry(
            &wrong_retained,
            &wrong_package,
            &fixture.home,
            &fixture.root,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut account(),
                &node::GraphBudget::new(),
            ),
        ),
        "registry-root-shape",
    );
    // A Cargo home reached through a link is the directory it names.
    let alias = fixture.root.join("home-alias");
    std::os::unix::fs::symlink(&fixture.home, &alias).expect("Cargo home alias");
    assert!(registry(
        &retained,
        &fixture.package,
        &alias,
        &fixture.root,
        &mut Budget::borrow(
            &Limits::standard(),
            &mut account(),
            &node::GraphBudget::new()
        )
    )
    .is_ok());
    fs::remove_file(&retained.marker).expect("remove extraction marker");
    assert!(fixture
        .observe(&retained, &mut account(), &node::GraphBudget::new())
        .is_err());
    let other = fixture.root.join("other-marker");
    fs::write(&other, b"{\"v\":1}").expect("same-content other marker");
    std::os::unix::fs::symlink(&other, &retained.marker).expect("marker alias");
    assert!(fixture
        .observe(&retained, &mut account(), &node::GraphBudget::new())
        .is_err());
}

#[test]
fn exact_shared_marker_grammar_schema_and_digest_drift_precede_entry_ownership() {
    let fixture = Fixture::new();
    let mut retained = fixture.retained();
    for bytes in [
        &b"{\"v\":1}"[..],
        &b"{\"\\u0076\":1}"[..],
        &b"[1]"[..],
        &b"{\"v\":0}"[..],
        &b"{\"v\":4294967296}"[..],
        &b"{\"v\":1.0}"[..],
        &b"{\"v\":1,\"v\":1}"[..],
        &b"{\"unknown\":[[[[]]]],\"v\":1}"[..],
        &b"{}"[..],
        &b"true"[..],
        &b"{\"v\":1} trailing"[..],
        &b"{\"\xff\":1}"[..],
    ] {
        fixture.write(bytes);
        retained.marker_digest = Sha256::digest(bytes).into();
        let legacy =
            crate::input::package::validate_registry_marker(&retained.marker, &mut account());
        let current = fixture.observe(&retained, &mut account(), &node::GraphBudget::new());
        assert_eq!(current.is_ok(), legacy.is_ok(), "marker bytes {bytes:?}");
        if let Err(crate::Error::Input { code, .. }) = legacy {
            policy(current, code);
        }
    }
    fixture.write(b"{\"v\":1} ");
    retained.marker_digest = Sha256::digest(b"{\"v\":1}").into();
    assert!(matches!(
        fixture.observe(&retained, &mut account(), &node::GraphBudget::new()),
        Err(stored::Error::Observation(observe::Error::Drift(
            "registry-marker-changed"
        )))
    ));
}

#[test]
fn opened_shortening_is_permitted_growth_refuses_and_initial_overflow_refuses_physically() {
    let fixture = Fixture::new();
    let retained = fixture.retained();
    fixture.write(b"{ \"v\" : 1 }");
    let mut shortened = account();
    acquire(
        &retained,
        &fixture.package,
        &fixture.home,
        &fixture.root,
        &mut Budget::borrow(
            &Limits::standard(),
            &mut shortened,
            &node::GraphBudget::new(),
        ),
        |path| {
            fs::write(path, b"{\"v\":1}").expect("shorter opened marker");
            Ok(())
        },
    )
    .expect("same saved-reader maximum, not exact pathname length");
    // A read that grew past its admitted buffer is a physical bound: it
    // refuses and latches the account.
    let mut allocation = account();
    assert!(matches!(
        acquire(
            &retained,
            &fixture.package,
            &fixture.home,
            &fixture.root,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new()
            ),
            |path| {
                fs::write(path, b"{\"v\":1} ").expect("grown opened marker");
                Ok(())
            }
        ),
        Err(stored::Error::Input(crate::Error::Limit {
            resource: "freshness file",
            limit: 7,
            actual: 8
        }))
    ));
    assert!(matches!(
        allocation.items(0),
        Err(crate::Error::InputAccountRefused)
    ));
    let mut bytes = b"{\"v\":1}".to_vec();
    bytes.resize(1024, b' ');
    fixture.write(&bytes);
    let mut retained = retained;
    retained.marker_digest = Sha256::digest(&bytes).into();
    let mut exact = account();
    fixture
        .observe(&retained, &mut exact, &node::GraphBudget::new())
        .expect("exact initial 1024 bound");
}

#[test]
fn original_exact_floors_cover_current_work_and_refusal_survives_a_fresh_graph_argument() {
    let fixture = Fixture::new();
    let retained = fixture.retained();
    let mut measured = account();
    fixture
        .observe(&retained, &mut measured, &node::GraphBudget::new())
        .expect("measure actual closure");
    let exact = measured.usage();
    let mut allocation = account();
    // The exact remainder records nothing; one less on either axis
    // records the exact total as an exceedance and the observation completes.
    fixture
        .observe(&retained, &mut allocation, &node::GraphBudget::new())
        .expect("admitted");
    assert_eq!(allocation.usage(), exact);
    // Only a genuine failure latches, and a fresh graph argument does not
    // revive the latched input account.
    let mut allocation = account();
    allocation.refuse();
    let spent = allocation.usage();
    assert!(matches!(
        fixture.observe(&retained, &mut allocation, &node::GraphBudget::new()),
        Err(stored::Error::Refused)
    ));
    assert_eq!(allocation.usage(), spent);
}

#[test]
fn current_graph_charges_native_attempts_and_only_actual_marker_hash() {
    let fixture = Fixture::new();
    let retained = fixture.retained();
    let mut measured = account();
    let graph = node::GraphBudget::new();
    fixture
        .observe(&retained, &mut measured, &graph)
        .expect("measure accounts");
    assert!(measured.usage().bytes > PRIOR.bytes);
}

#[test]
fn parser_envelope_is_admitted_before_the_parser_runs() {
    for bytes in [&b"{\"v\":1}"[..], &b"{\"\\u0076\":1}"[..]] {
        let mut allocation = account();
        super::parser::admit(
            bytes,
            &mut Budget::borrow(
                &Limits::standard(),
                &mut allocation,
                &node::GraphBudget::new(),
            ),
        )
        .expect("pinned parser allowance");
        assert_ne!(allocation.usage(), PRIOR);
        crate::input::package::marker(bytes).expect("shared Marker parser");
    }
}
