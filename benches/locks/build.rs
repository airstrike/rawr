//! Embed what this build measures: the revision, whether the tree had
//! changes and the digest of those changes, and the profile it was built
//! with. A result then names its build, not the checkout at run time.

use std::path::Path;
use std::process::Command;

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let git = |args: &[&str]| -> Option<String> {
        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(args)
            .output()
            .ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
    };
    let revision = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    let changes = git(&["diff", "HEAD"]).unwrap_or_default();
    // The digest of the uncommitted changes, by git's own object hash, so a
    // dirty build is told apart from another dirty build of one revision.
    let digest = if changes.is_empty() {
        String::new()
    } else {
        let mut hash = Command::new("git")
            .args(["hash-object", "--stdin"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .expect("git runs, as it did for the revision");
        {
            use std::io::Write as _;
            let mut stdin = hash.stdin.take().expect("stdin is piped");
            stdin
                .write_all(changes.as_bytes())
                .expect("git reads the changes");
        }
        let output = hash.wait_with_output().expect("git hashes the changes");
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    };
    // OUT_DIR is <target>/<profile>/build/<package>-<hash>/out, and the
    // profile's directory is its name ("debug" for `dev`).
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    let profile = Path::new(&out)
        .ancestors()
        .nth(3)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    println!("cargo:rustc-env=LOCK_BENCH_REVISION={revision}");
    println!("cargo:rustc-env=LOCK_BENCH_DIRTY={}", !changes.is_empty());
    println!("cargo:rustc-env=LOCK_BENCH_CHANGES={digest}");
    println!("cargo:rustc-env=LOCK_BENCH_PROFILE={profile}");
    println!(
        "cargo:rustc-env=LOCK_BENCH_OPT_LEVEL={}",
        std::env::var("OPT_LEVEL").unwrap_or_default()
    );
    // Build again when the revision, the index or the measured sources move.
    for args in [
        ["rev-parse", "--git-path", "HEAD"],
        ["rev-parse", "--git-path", "index"],
    ] {
        if let Some(path) = git(&args) {
            println!("cargo:rerun-if-changed={}", root.join(path).display());
        }
    }
    for source in ["src", "importer/src", "Cargo.toml", "benches/locks"] {
        println!("cargo:rerun-if-changed={}", root.join(source).display());
    }
}
