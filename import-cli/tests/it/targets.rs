//! A package's binaries as Cargo lists them: a `[[bin]]` outside `src/`
//! and a binary under `src/bin/` run by their files and by `--bin`, with
//! the dependencies Cargo links on the host and the package's profile.
//! Build the driver first, as for `plain`.

use std::path::Path;
use std::process::{Command, Output};

use crate::support::Fixture;

static TARGETS: Fixture = Fixture::new("bin-targets");

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `rawr-import` with `arguments` in the package at `root`.
fn rawr(root: &Path, home: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rawr-import"))
        .args(arguments)
        .current_dir(root)
        .env("RAWR_HOME", home)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .env_remove("RAWR_VERBOSE")
        .output()
        .unwrap()
}

/// Each binary prints what Cargo's build of it prints, by its file and by
/// `--bin`: through a dependency of a `[target.'cfg(..)']` table that holds
/// on the host, while a table for wasm32, whose crate refuses to build
/// here, stays out (#1214). Its marked modules are beside it (#1116). A
/// `[[bin]]` file runs as the package's binary, not alone (#1175), and the
/// package's `panic = "abort"` aborts its panic as Cargo's build does
/// (#1176).
#[test]
fn a_packages_binaries_run_with_its_host_dependencies_and_profile() {
    let root = TARGETS.lease(&[]);
    let home = crate::support::scratch().join("rawr-home");
    let built = Command::new("cargo")
        .args(["build", "--quiet", "--bins"])
        .current_dir(&root)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .unwrap();
    assert!(built.status.success(), "{}", text(&built.stderr));
    let native = |name: &str, arguments: &[&str]| {
        Command::new(root.join("target/debug").join(name))
            .args(arguments)
            .output()
            .unwrap()
    };

    for (name, file) in [
        ("tool", "exercises/tool.rs"),
        ("app", "src/bin/app/main.rs"),
    ] {
        let expected = native(name, &[]);
        assert!(expected.status.success(), "{}", text(&expected.stderr));
        for arguments in [&["run", file][..], &["run", "--bin", name]] {
            let run = rawr(&root, &home, arguments);
            assert!(
                run.status.success(),
                "{arguments:?}\nstdout:\n{}\nstderr:\n{}",
                text(&run.stdout),
                text(&run.stderr)
            );
            assert_eq!(text(&run.stdout), text(&expected.stdout), "{arguments:?}");
        }
    }

    let aborted = native("tool", &["panic"]);
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        assert_eq!(
            aborted.status.signal(),
            Some(6),
            "{}",
            text(&aborted.stderr)
        );
    }
    let run = rawr(&root, &home, &["run", "exercises/tool.rs", "--", "panic"]);
    // SIGABRT, as a shell gives it.
    assert_eq!(run.status.code(), Some(128 + 6), "{}", text(&run.stderr));
    assert_eq!(text(&run.stdout), text(&aborted.stdout));
    assert!(
        text(&run.stderr).contains("asked to panic"),
        "{}",
        text(&run.stderr)
    );
}
