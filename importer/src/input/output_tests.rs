//! Cargo artifact messages keep required outputs strict and debug files optional.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::{json, Value};

use super::build::Evidence;
use crate::cargo::{toolchain, Limits};
use crate::{Error, Result};

struct Fixture {
    root: PathBuf,
    output: PathBuf,
    message: Value,
}

impl Fixture {
    fn new(kind: &str) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-output-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let (filename, dep_info) = match kind {
            "custom-build" => (
                "build/probe-abc/build-script-build",
                "build/probe-abc/build_script_build-abc.d",
            ),
            "proc-macro" => ("deps/libprobe-abc.dylib", "deps/probe-abc.d"),
            "lib" => ("deps/libprobe-abc.rlib", "deps/probe-abc.d"),
            _ => panic!("fixture supports libraries, proc macros and build scripts"),
        };
        let output = root.join(filename);
        fs::create_dir_all(output.parent().unwrap()).unwrap();
        fs::write(&output, b"artifact").unwrap();
        fs::write(root.join(dep_info), b"artifact: source.rs\n").unwrap();
        fs::write(
            root.join("Cargo.toml"),
            b"[package]\nname='probe'\nversion='0.1.0'\n",
        )
        .unwrap();
        fs::write(root.join("source.rs"), b"fn main() {}\n").unwrap();
        let message = json!({
            "reason": "compiler-artifact",
            "package_id": "path+file:///fixture#probe@0.1.0",
            "manifest_path": root.join("Cargo.toml"),
            "target": {
                "kind": [kind],
                "crate_types": [if kind == "custom-build" { "bin" } else { kind }],
                "name": "probe",
                "src_path": root.join("source.rs"),
            },
            "features": [],
            "profile": {
                "opt_level": "0", "debuginfo": 2, "debug_assertions": true,
                "overflow_checks": true, "test": false,
            },
            "fresh": false,
            "filenames": [output],
            "executable": null,
        });
        Self {
            root,
            output,
            message,
        }
    }

    fn parse(&self) -> Result<Evidence> {
        let mut bytes = serde_json::to_vec(&self.message).unwrap();
        bytes.extend_from_slice(b"\n{\"reason\":\"build-finished\",\"success\":true}\n");
        Evidence::parse(
            &bytes,
            Box::default(),
            toolchain::supported_cargo_commit(),
            &Limits::standard(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn missing_debug_companions_are_optional_for_build_scripts_and_libraries() {
    for kind in ["custom-build", "proc-macro", "lib"] {
        let mut fixture = Fixture::new(kind);
        for suffix in ["dSYM", "dwp", "pdb"] {
            let missing = fixture.output.with_extension(suffix);
            fixture.message["filenames"] = json!([fixture.output, missing]);
            for fresh in [false, true] {
                fixture.message["fresh"] = json!(fresh);
                let evidence = fixture
                    .parse()
                    .expect("missing debug info is supplementary");
                assert_eq!(
                    evidence.artifacts[0].outputs.as_ref(),
                    [fixture.output.clone()]
                );
            }
        }
    }
}

#[test]
fn present_debug_files_and_bundles_remain_canonical_outputs() {
    let mut fixture = Fixture::new("lib");
    let bundle = fixture.root.join("probe.dSYM");
    let dwp = fixture.root.join("probe.dwp");
    let pdb = fixture.root.join("probe.pdb");
    fs::create_dir(&bundle).unwrap();
    fs::write(&dwp, b"debug").unwrap();
    fs::write(&pdb, b"debug").unwrap();
    let mut expected = vec![
        fixture.output.clone(),
        bundle.clone(),
        dwp.clone(),
        pdb.clone(),
    ];
    // A symlinked output must retain its resolved name, not its Cargo spelling.
    #[cfg(unix)]
    let bundle = {
        let link = fixture.root.join("linked.dSYM");
        std::os::unix::fs::symlink(&bundle, &link).unwrap();
        link
    };
    fixture.message["filenames"] = json!([fixture.output, bundle, dwp, pdb, fixture.output]);
    fixture.message["executable"] = json!(fixture.output);
    let evidence = fixture.parse().unwrap();
    expected.sort_unstable();
    assert_eq!(evidence.artifacts[0].outputs.as_ref(), expected);
}

#[test]
fn missing_required_artifacts_still_fail() {
    for kind in ["custom-build", "proc-macro", "lib"] {
        let fixture = Fixture::new(kind);
        fs::remove_file(&fixture.output).unwrap();
        assert_missing(fixture.parse(), &fixture.output);
    }
    let mut fixture = Fixture::new("lib");
    for filename in [
        "libprobe-abc.rmeta",
        "probe.dll",
        "probe.so",
        "probe.a",
        "probe.map",
    ] {
        let missing = fixture.root.join(filename);
        fixture.message["filenames"] = json!([fixture.output, missing]);
        assert_missing(fixture.parse(), &missing);
    }
}

#[test]
fn reported_executable_is_required_even_with_a_debug_extension() {
    let mut fixture = Fixture::new("custom-build");
    for filename in [
        "missing-executable",
        "missing.dSYM",
        "missing.dwp",
        "missing.pdb",
    ] {
        let missing = fixture.root.join(filename);
        fixture.message["executable"] = json!(missing);
        fixture.message["filenames"] = json!([fixture.output]);
        assert_missing(fixture.parse(), &missing);
        // Being optional in filenames must not waive the executable field.
        fixture.message["filenames"] = json!([fixture.output, missing]);
        assert_missing(fixture.parse(), &missing);
    }
}

#[cfg(unix)]
#[test]
fn debug_companion_inspection_errors_are_not_treated_as_absence() {
    let mut fixture = Fixture::new("lib");
    let looping = fixture.root.join("loop.dSYM");
    std::os::unix::fs::symlink(&looping, &looping).unwrap();
    fixture.message["filenames"] = json!([fixture.output, looping]);
    let error = fixture
        .parse()
        .err()
        .expect("a symlink loop is an inspection error");
    assert!(matches!(error, Error::Io { path, source, .. }
        if path == looping && source.kind() != std::io::ErrorKind::NotFound));
}

fn assert_missing(result: Result<Evidence>, expected: &Path) {
    let error = result.err().expect("required Cargo output must exist");
    assert!(matches!(error, Error::Io { path, source, .. }
        if path == expected && source.kind() == std::io::ErrorKind::NotFound));
}
