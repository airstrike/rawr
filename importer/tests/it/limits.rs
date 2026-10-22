use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::cargo::{toolchain, Limits};
use rawr_import::input::declaration::Declarations;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Layout(PathBuf);

impl Layout {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-saved-output-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        let out = root.join("build/package-hash/out");
        fs::create_dir_all(&out).expect("the pinned saved-output layout is created");
        Self(fs::canonicalize(out).expect("the saved-output OUT_DIR is canonical"))
    }

    fn write(&self, bytes: &[u8]) {
        fs::write(
            self.0
                .parent()
                .expect("OUT_DIR has a parent")
                .join("output"),
            bytes,
        )
        .expect("the saved output is written");
    }
}

impl Drop for Layout {
    fn drop(&mut self) {
        let root = self
            .0
            .ancestors()
            .nth(3)
            .expect("the test layout has a root")
            .to_path_buf();
        let _ = fs::remove_dir_all(root);
    }
}

#[test]
fn pinned_saved_output_decodes_ordered_directives_with_strict_binding() {
    let layout = Layout::new();
    layout.write(
        b"cargo output without a directive is harmless\n\
          cargo:rerun-if-changed=relative.txt\n\
          cargo::rerun-if-changed=/absolute.txt\n\
          cargo:rerun-if-env-changed=VISIBLE\n\
          cargo:rustc-cfg=fixture\n",
    );
    let declarations = Declarations::read(
        toolchain::supported_cargo_commit(),
        "path+file:///fixture#0.1.0",
        &layout.0,
        &Limits::standard(),
    )
    .expect("the exact saved output decodes");
    assert_eq!(declarations.directive_count(), 4);
    assert_eq!(declarations.path_count(), 2);
    assert_eq!(declarations.environment_count(), 1);

    let skew = Declarations::read(
        "0000000000000000000000000000000000000000",
        "path+file:///fixture#0.1.0",
        &layout.0,
        &Limits::standard(),
    )
    .err()
    .expect("Cargo commit skew rejects");
    assert!(format!("{skew}").contains("cargo-commit-skew"));

    let wrong = layout.0.parent().expect("OUT_DIR has a parent");
    let layout_error = Declarations::read(
        toolchain::supported_cargo_commit(),
        "path+file:///fixture#0.1.0",
        wrong,
        &Limits::standard(),
    )
    .err()
    .expect("layout skew rejects");
    assert!(format!("{layout_error}").contains("saved-output-layout"));
}

#[test]
fn malformed_and_truncated_saved_outputs_reject_before_proof() {
    let layout = Layout::new();
    layout.write(b"cargo:rerun-if-changed");
    let truncated = read_error(&layout);
    assert!(truncated.contains("truncated-saved-output"));

    layout.write(b"cargo:rerun-if-changed\n");
    let malformed = read_error(&layout);
    assert!(malformed.contains("malformed-saved-output"));

    layout.write(b"cargo::not-an-instruction\n");
    let malformed_prefix = read_error(&layout);
    assert!(malformed_prefix.contains("malformed-saved-output"));
}

fn read_error(layout: &Layout) -> String {
    Declarations::read(
        toolchain::supported_cargo_commit(),
        "path+file:///fixture#0.1.0",
        &layout.0,
        &Limits::standard(),
    )
    .err()
    .map(|error| format!("{error}"))
    .expect("invalid saved output rejects")
}
