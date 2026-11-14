//! The suite's one `RAWR_RUNTIME_HOME`, with rawr's runtime compiled in it
//! before any test's run starts its clock. On a cold key (a new rawr, a new
//! toolchain) that compile is the largest single build a run waits on, and
//! one test pays it for the suite: left to whichever test's run reached its
//! runner build first, it counted against that run's limit, while the
//! others waited on its lock against theirs (#1049). The first test to ask
//! compiles it here, and every other test waits for that and reuses it.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// A program with one path dependency: the least a runner links rawr's
/// runtime for.
const FILES: &[(&str, &str)] = &[
    (
        "Cargo.toml",
        "[workspace]\n[package]\nname='runtime-warmup'\nversion='0.1.0'\nedition='2024'\n\
         [dependencies]\nwarmup-unit={path='unit'}\n",
    ),
    (
        "unit/Cargo.toml",
        "[package]\nname='warmup-unit'\nversion='0.1.0'\nedition='2024'\n",
    ),
    ("unit/src/lib.rs", "pub fn one() -> i64 {\n    1\n}\n"),
    (
        "src/main.rs",
        "// rawr\nfn main() {\n    println!(\"{}\", warmup_unit::one());\n}\n",
    ),
];

/// The suite's `RAWR_RUNTIME_HOME`, its shared `RAWR_HOME`, once rawr's
/// runtime for the ambient toolchain is compiled there. A test whose
/// project names another toolchain still compiles that one's runtime in its
/// own run.
pub fn home() -> PathBuf {
    static HOME: OnceLock<PathBuf> = OnceLock::new();
    HOME.get_or_init(|| {
        let home = super::scratch().join("rawr-home");
        warm(&super::scratch().join("runtime-warmup"), &home);
        home
    })
    .clone()
}

/// Run the warm-up program in `root` with `home` as rawr's home: a warm
/// key costs its cached run, a cold one compiles the runtime.
fn warm(root: &Path, home: &Path) {
    for (path, text) in FILES {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if fs::read_to_string(&path).ok().as_deref() != Some(*text) {
            fs::write(&path, text).unwrap();
        }
    }
    let output = crate::support::rawr_import()
        .arg("run")
        .arg(root)
        .env("RAWR_HOME", home)
        .env("RAWR_RUNTIME_HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap();
    assert!(
        output.status.success() && output.stdout == b"1\n",
        "rawr's runtime did not build for the suite:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
