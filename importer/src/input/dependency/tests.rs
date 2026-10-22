use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{environment, Dependency};
use crate::cargo::{toolchain, Limits};
use crate::input::{Allocation, Usage};
use crate::Error;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn open() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-dep-info-scan-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("isolated dep-info directory");
        let path = fs::canonicalize(path).expect("canonical dep-info fixture root");
        fs::write(path.join("a.rs"), b"pub fn a() {}\n").expect("first source");
        fs::write(path.join("b file.rs"), b"pub fn b() {}\n").expect("escaped source");
        Self(path)
    }

    fn read(
        &self,
        text: &str,
        limits: &Limits,
        allocation: &mut Allocation,
    ) -> crate::Result<Dependency> {
        let path = self.0.join("target.d");
        fs::write(&path, text).expect("fixture dep-info");
        Dependency::read(
            toolchain::supported_cargo_commit(),
            &path,
            &self.0,
            limits,
            allocation,
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("retire dep-info fixture");
    }
}

#[test]
fn legacy_reader_retains_canonical_sorted_sources_and_raw_environment() {
    let fixture = Fixture::open();
    let limits = Limits::standard();
    let dependency = fixture.read(
        "target: b\\ file.rs a.rs a.rs\n# env-dep:SET=a\\\nb\n# env-dep:EMPTY=\n# env-dep:UNSET\n",
        &limits, &mut Allocation::new(&limits),
    ).expect("the shared grammar feeds the existing source resolver");
    assert_eq!(
        dependency.sources.as_ref(),
        [
            fixture
                .0
                .join("a.rs")
                .canonicalize()
                .expect("first canonical source"),
            fixture
                .0
                .join("b file.rs")
                .canonicalize()
                .expect("second canonical source"),
        ]
    );
    assert_eq!(
        dependency.environment.as_ref(),
        [
            (Box::from("EMPTY"), Some(Box::from(""))),
            (Box::from("SET"), Some(Box::from("a\\"))),
            (Box::from("UNSET"), None),
        ]
    );
}

#[test]
fn reader_preserves_environment_and_path_failure_order() {
    let fixture = Fixture::open();
    let limits = Limits::standard();
    for (text, code, reason) in [
        (
            "missing rule\n# env-dep:\n\\\n",
            "malformed-dep-info",
            "trailing line escape",
        ),
        (
            "missing rule\n# env-dep:\n",
            "malformed-env-dep",
            "# env-dep:",
        ),
        (
            "missing rule\n# env-dep:K\n# env-dep:K\n",
            "duplicate-env-dep",
            "K",
        ),
    ] {
        let error = fixture
            .read(text, &limits, &mut Allocation::new(&limits))
            .err()
            .expect("the fixture rejects");
        assert!(
            matches!(error, Error::Input { code: actual, reason: actual_reason }
            if actual == code && actual_reason.as_ref() == reason)
        );
    }
    let error = fixture
        .read(
            "target: missing.rs a\\",
            &limits,
            &mut Allocation::new(&limits),
        )
        .err()
        .expect("source resolution fails at the preceding boundary");
    assert!(matches!(
        error,
        Error::Input {
            code: "missing-dep-info-source",
            ..
        }
    ));
    // The token length is an accounting target. Crossing it records a
    // truthful high water and the reader continues to the trailing syntax.
    let mut tiny = limits;
    tiny.string_bytes = 1;
    let mut allocation = Allocation::new(&tiny);
    let error = fixture
        .read("target: aa\\", &tiny, &mut allocation)
        .err()
        .expect("the trailing syntax still fails");
    assert!(!matches!(error, Error::Limit { .. }), "{error:?}");
    let axis = *allocation
        .exceedances()
        .axis("dep-info token bytes")
        .expect("the token crossing is recorded");
    assert_eq!((axis.target, axis.high_water), (1, 2));
}

#[test]
fn duplicate_environment_storage_spends_the_original_account_before_rejecting() {
    let limits = Limits::standard();
    let prior = Usage {
        items: 7,
        bytes: 23,
    };
    for record in ["# env-dep:K\n# env-dep:K\n", "# env-dep:K=\n# env-dep:K=\n"] {
        let mut allocation = Allocation::for_proof(prior, &limits);
        assert!(matches!(
            environment(record, &limits, &mut allocation),
            Err(Error::Input {
                code: "duplicate-env-dep",
                ..
            })
        ));
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + if record.contains('=') { 6 } else { 4 },
                bytes: prior.bytes + 2,
            }
        );
    }
    let record = "# env-dep:K=V\n";
    for floor in 0..=1 {
        let mut limits = limits;
        limits.input_proof_bytes = prior.bytes + 2 - floor;
        let mut allocation = Allocation::for_proof(prior, &limits);
        // The exact remainder records nothing; one less byte records the
        // exact total as an exceedance and the environment is still admitted.
        environment(record, &limits, &mut allocation)
            .expect("accounting targets never refuse the environment");
        assert_eq!(
            allocation.usage(),
            Usage {
                items: prior.items + 4,
                bytes: prior.bytes + 2
            }
        );
        let exceedances = allocation.exceedances();
        if floor == 0 {
            assert!(!exceedances.is_exceeded());
        } else {
            let bytes = exceedances
                .axis(crate::input::INPUT_BYTES_RESOURCE)
                .expect("one less byte records the byte axis");
            assert_eq!(bytes.target, (prior.bytes + 1) as u64);
            assert_eq!(bytes.high_water, (prior.bytes + 2) as u64);
            assert_eq!(exceedances.axes().count(), 1);
        }
        allocation
            .items(0)
            .expect("an exceedance never latches the account");
        // Only a genuine failure latches, and the latch is terminal.
        allocation.refuse();
        assert!(matches!(
            environment(record, &limits, &mut allocation),
            Err(Error::InputAccountRefused)
        ));
    }
}
